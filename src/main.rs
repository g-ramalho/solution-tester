use std::{
    cmp::min,
    collections::BTreeMap,
    fmt::Display,
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use clap::Parser;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Wrap},
};
use walkdir::WalkDir;

fn parse_benchmark_runs(value: &str) -> Result<usize, String> {
    let runs = value
        .parse()
        .map_err(|_| "benchmark runs must be a positive integer".to_owned())?;
    if runs < 2 {
        Err("benchmark runs must be at least 2".to_owned())
    } else {
        Ok(runs)
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "solution-tester",
    version,
    about = "Run a solution against recursively discovered .in/.sol test pairs"
)]
struct Cli {
    /// Executable to test.
    executable: PathBuf,

    /// Directory containing test files.
    directory: PathBuf,

    /// Continue running tests after a failure.
    #[arg(long)]
    keep_going: bool,

    /// Maximum runtime for each test, in seconds.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    timeout: u64,

    /// Run every test multiple times and report benchmark timing statistics.
    #[arg(
        long,
        value_name = "RUNS",
        num_args = 0..=1,
        default_missing_value = "10",
        value_parser = parse_benchmark_runs
    )]
    benchmark: Option<usize>,
}

#[derive(Debug, Clone)]
struct TestCase {
    key: PathBuf,
    input_path: Option<PathBuf>,
    expected_path: Option<PathBuf>,
}

impl TestCase {
    fn display_name(&self) -> String {
        self.key.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TestStatus {
    Passed,
    MissingInput,
    MissingExpected,
    ReadError,
    SpawnError,
    WriteError,
    TimedOut,
    NonZeroExit,
    OutputMismatch,
}

impl TestStatus {
    fn is_passed(&self) -> bool {
        matches!(self, Self::Passed)
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Passed => "passed",
            Self::MissingInput => "missing input",
            Self::MissingExpected => "missing answer",
            Self::ReadError => "file read error",
            Self::SpawnError => "spawn error",
            Self::WriteError => "stdin write error",
            Self::TimedOut => "timed out",
            Self::NonZeroExit => "non-zero exit",
            Self::OutputMismatch => "wrong answer",
        }
    }
}

impl Display for TestStatus {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.label())
    }
}

#[derive(Debug, Clone)]
struct TestResult {
    name: String,
    status: TestStatus,
    duration: Duration,
    benchmark: Option<BenchmarkStats>,
    expected: String,
    actual: String,
    stderr: String,
    exit_code: Option<i32>,
    detail: String,
}

impl TestResult {
    fn passed(&self) -> bool {
        self.status.is_passed()
    }

    fn display_duration(&self) -> Duration {
        self.benchmark
            .as_ref()
            .map_or(self.duration, |benchmark| benchmark.timing.average)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BenchmarkStats {
    runs: usize,
    timing: TimingStats,
    durations: Vec<Duration>,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_paths(&cli)?;

    let tests = discover_tests(&cli.directory)?;
    let results = run_tests(
        &cli.executable,
        &tests,
        Duration::from_secs(cli.timeout),
        cli.keep_going,
        cli.benchmark,
    );

    run_results_ui(&results)?;

    if results.iter().any(|result| !result.passed()) {
        std::process::exit(1);
    }
    Ok(())
}

fn run_tests(
    executable: &Path,
    tests: &[TestCase],
    timeout: Duration,
    keep_going: bool,
    benchmark_runs: Option<usize>,
) -> Vec<TestResult> {
    let mut results = Vec::with_capacity(tests.len());
    for test in tests {
        let result = benchmark_runs.map_or_else(
            || run_test(executable, test, timeout),
            |runs| run_benchmark(executable, test, timeout, runs),
        );
        let failed = !result.passed();
        results.push(result);
        if failed && !keep_going {
            break;
        }
    }
    results
}

fn validate_paths(cli: &Cli) -> Result<()> {
    if !cli.executable.is_file() {
        bail!(
            "executable path is not a file: {}",
            cli.executable.display()
        );
    }
    if !cli.directory.is_dir() {
        bail!(
            "test directory is not a directory: {}",
            cli.directory.display()
        );
    }
    Ok(())
}

fn discover_tests(root: &Path) -> Result<Vec<TestCase>> {
    let mut pairs: BTreeMap<PathBuf, (Option<PathBuf>, Option<PathBuf>)> = BTreeMap::new();

    for entry in WalkDir::new(root).follow_links(false) {
        let entry = entry.with_context(|| format!("walking {}", root.display()))?;
        if !entry.file_type().is_file() {
            continue;
        }

        let path = entry.into_path();
        let Some(extension) = path.extension().and_then(|extension| extension.to_str()) else {
            continue;
        };
        if extension != "in" && extension != "sol" {
            continue;
        }

        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("strip test root from {}", path.display()))?;
        let mut key = relative.to_path_buf();
        key.set_extension("");
        let pair = pairs.entry(key).or_default();
        if extension == "in" {
            pair.0 = Some(path);
        } else {
            pair.1 = Some(path);
        }
    }

    Ok(pairs
        .into_iter()
        .map(|(key, (input_path, expected_path))| TestCase {
            key,
            input_path,
            expected_path,
        })
        .collect())
}

fn run_test(executable: &Path, test: &TestCase, timeout: Duration) -> TestResult {
    let started = Instant::now();
    let name = test.display_name();

    let Some(input_path) = &test.input_path else {
        return failure_result(
            name,
            TestStatus::MissingInput,
            started.elapsed(),
            "no matching .in file",
        );
    };
    let Some(expected_path) = &test.expected_path else {
        return failure_result(
            name,
            TestStatus::MissingExpected,
            started.elapsed(),
            "no matching .sol file",
        );
    };

    let input = match fs::read(input_path) {
        Ok(input) => input,
        Err(error) => {
            return failure_result(
                name,
                TestStatus::ReadError,
                started.elapsed(),
                format!("could not read {}: {error}", input_path.display()),
            );
        }
    };
    let expected = match fs::read(expected_path) {
        Ok(expected) => display_bytes(&expected),
        Err(error) => {
            return failure_result(
                name,
                TestStatus::ReadError,
                started.elapsed(),
                format!("could not read {}: {error}", expected_path.display()),
            );
        }
    };

    let mut child = match Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return failure_result(
                name,
                TestStatus::SpawnError,
                started.elapsed(),
                format!("could not start {}: {error}", executable.display()),
            );
        }
    };

    let stdout_reader = child.stdout.take().map(start_reader);
    let stderr_reader = child.stderr.take().map(start_reader);

    if let Some(mut stdin) = child.stdin.take()
        && let Err(error) = stdin.write_all(&input)
    {
        let _ = child.kill();
        let _ = child.wait();
        let _ = join_reader(stdout_reader);
        let _ = join_reader(stderr_reader);
        return failure_result(
            name,
            TestStatus::WriteError,
            started.elapsed(),
            format!("could not write test input: {error}"),
        );
    }

    let timed_out = wait_for_child(&mut child, timeout);
    let actual_bytes = join_reader(stdout_reader);
    let stderr_bytes = join_reader(stderr_reader);
    let (actual_bytes, stderr_bytes) = match (actual_bytes, stderr_bytes) {
        (Ok(actual), Ok(stderr)) => (actual, stderr),
        (Err(error), _) | (_, Err(error)) => {
            return failure_result(
                name,
                TestStatus::ReadError,
                started.elapsed(),
                format!("could not read process output: {error}"),
            );
        }
    };
    let actual = display_bytes(&actual_bytes);
    let stderr = display_bytes(&stderr_bytes);
    let duration = started.elapsed();

    if timed_out {
        return TestResult {
            name,
            status: TestStatus::TimedOut,
            duration,
            benchmark: None,
            expected,
            actual,
            stderr,
            exit_code: None,
            detail: format!("process exceeded the {} second timeout", timeout.as_secs()),
        };
    }

    let exit_code = child
        .try_wait()
        .ok()
        .flatten()
        .and_then(|status| status.code());
    if exit_code != Some(0) {
        return TestResult {
            name,
            status: TestStatus::NonZeroExit,
            duration,
            benchmark: None,
            expected,
            actual,
            stderr,
            exit_code,
            detail: "the executable exited unsuccessfully".to_owned(),
        };
    }

    if normalize_output(&actual) == normalize_output(&expected) {
        TestResult {
            name,
            status: TestStatus::Passed,
            duration,
            benchmark: None,
            expected,
            actual,
            stderr,
            exit_code,
            detail: "output matches the expected answer".to_owned(),
        }
    } else {
        TestResult {
            name,
            status: TestStatus::OutputMismatch,
            duration,
            benchmark: None,
            expected: expected.clone(),
            actual: actual.clone(),
            stderr,
            exit_code,
            detail: mismatch_detail(&expected, &actual),
        }
    }
}

fn wait_for_child(child: &mut Child, timeout: Duration) -> bool {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return false,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return true;
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(_) => return false,
        }
    }
}

fn start_reader(mut output: impl Read + Send + 'static) -> JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

fn join_reader(reader: Option<JoinHandle<io::Result<Vec<u8>>>>) -> io::Result<Vec<u8>> {
    let reader = reader.ok_or_else(|| io::Error::other("missing process output pipe"))?;
    reader
        .join()
        .map_err(|_| io::Error::other("process output reader panicked"))?
}

fn failure_result(
    name: String,
    status: TestStatus,
    duration: Duration,
    detail: impl Into<String>,
) -> TestResult {
    TestResult {
        name,
        status,
        duration,
        benchmark: None,
        expected: String::new(),
        actual: String::new(),
        stderr: String::new(),
        exit_code: None,
        detail: detail.into(),
    }
}

fn display_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn normalize_output(output: &str) -> String {
    output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

fn mismatch_detail(expected: &str, actual: &str) -> String {
    let normalized_expected = normalize_output(expected);
    let normalized_actual = normalize_output(actual);
    let expected_lines: Vec<_> = normalized_expected.lines().collect();
    let actual_lines: Vec<_> = normalized_actual.lines().collect();
    let first_difference = (0..min(expected_lines.len(), actual_lines.len()))
        .find(|index| expected_lines[*index] != actual_lines[*index]);

    match first_difference {
        Some(line) => format!("first mismatch at line {}", line + 1),
        None if expected_lines.len() != actual_lines.len() => format!(
            "different number of output lines (expected {}, got {})",
            expected_lines.len(),
            actual_lines.len()
        ),
        None => "output differs".to_owned(),
    }
}

fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TimingStats {
    total: Duration,
    average: Duration,
    p90: Duration,
}

fn timing_stats(results: &[TestResult]) -> Option<TimingStats> {
    timing_stats_for_durations(results.iter().map(|result| result.duration))
}

fn timing_stats_for_durations(
    durations: impl IntoIterator<Item = Duration>,
) -> Option<TimingStats> {
    let mut durations: Vec<_> = durations.into_iter().collect();
    if durations.is_empty() {
        return None;
    }
    durations.sort_unstable();
    let total = durations.iter().copied().sum();
    let average = total / durations.len() as u32;
    let p90_index = (9 * durations.len() - 1) / 10;

    Some(TimingStats {
        total,
        average,
        p90: durations[p90_index],
    })
}

fn benchmark_timing_stats(results: &[TestResult]) -> Option<TimingStats> {
    timing_stats_for_durations(
        results
            .iter()
            .filter_map(|result| result.benchmark.as_ref())
            .flat_map(|benchmark| benchmark.durations.iter().copied()),
    )
}

fn benchmark_info(results: &[TestResult]) -> Option<(usize, usize)> {
    let benchmark = results
        .iter()
        .find_map(|result| result.benchmark.as_ref())?;
    let executions = results
        .iter()
        .filter_map(|result| result.benchmark.as_ref())
        .map(|benchmark| benchmark.durations.len())
        .sum();
    Some((benchmark.runs, executions))
}

fn format_benchmark_difference(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}

fn benchmark_difference(benchmark: &BenchmarkStats, index: usize) -> (String, Color) {
    let Some(previous) = index.checked_sub(1) else {
        return ("-".to_owned(), Color::DarkGray);
    };

    let current = benchmark.durations[index];
    let previous = benchmark.durations[previous];
    match current.cmp(&previous) {
        std::cmp::Ordering::Less => (
            format!("-{}", format_benchmark_difference(previous - current)),
            Color::Green,
        ),
        std::cmp::Ordering::Greater => (
            format!("+{}", format_benchmark_difference(current - previous)),
            Color::Red,
        ),
        std::cmp::Ordering::Equal => (format_benchmark_difference(Duration::ZERO), Color::DarkGray),
    }
}

fn run_benchmark(executable: &Path, test: &TestCase, timeout: Duration, runs: usize) -> TestResult {
    assert!(runs > 0, "benchmark runs must be greater than zero");

    let mut durations = Vec::with_capacity(runs);
    let mut representative = None;
    for _ in 0..runs {
        let result = run_test(executable, test, timeout);
        durations.push(result.duration);

        let should_replace = representative
            .as_ref()
            .is_none_or(|current: &TestResult| current.passed() && !result.passed());
        if should_replace {
            representative = Some(result);
        }
    }

    let timing = timing_stats_for_durations(durations.iter().copied())
        .expect("benchmark has at least one run");
    let mut result = representative.expect("benchmark has a representative result");
    result.benchmark = Some(BenchmarkStats {
        runs,
        timing,
        durations,
    });
    result
}

fn run_results_ui(results: &[TestResult]) -> Result<()> {
    enable_raw_mode().context("enable raw terminal mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error).context("enter alternate screen");
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    let ui_result = run_ui_loop(&mut terminal, results);
    let disable_result = disable_raw_mode().context("restore terminal mode");
    let leave_result =
        execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen");
    let cursor_result = terminal.show_cursor().context("show terminal cursor");

    ui_result?;
    disable_result?;
    leave_result?;
    cursor_result
}

fn run_ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    results: &[TestResult],
) -> Result<()> {
    let mut selected = 0;
    let mut detail = false;
    let mut detail_scroll = 0;
    let mut benchmark_visible = true;
    let mut list_state = ListState::default();
    list_state.select((!results.is_empty()).then_some(selected));

    loop {
        terminal.draw(|frame| {
            if detail {
                render_detail(
                    frame,
                    results.get(selected),
                    detail_scroll,
                    benchmark_visible,
                );
            } else {
                render_results(frame, results, &mut list_state);
            }
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };

        if handle_key(
            key,
            results.len(),
            &mut selected,
            &mut detail,
            &mut detail_scroll,
            &mut benchmark_visible,
        ) {
            break;
        }
        list_state.select((!results.is_empty()).then_some(selected));
    }
    Ok(())
}

fn handle_key(
    key: KeyEvent,
    result_count: usize,
    selected: &mut usize,
    detail: &mut bool,
    detail_scroll: &mut u16,
    benchmark_visible: &mut bool,
) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => {
            if *detail {
                *detail = false;
                *detail_scroll = 0;
            } else {
                return true;
            }
        }
        (KeyCode::Enter, _) if result_count > 0 => {
            *detail = !*detail;
            *detail_scroll = 0;
        }
        (KeyCode::Char('e'), _) if *detail => {
            *benchmark_visible = !*benchmark_visible;
            *detail_scroll = 0;
        }
        (KeyCode::Up | KeyCode::Char('k'), _) if *detail => {
            *detail_scroll = detail_scroll.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), _) if *detail => {
            *detail_scroll = detail_scroll.saturating_add(1);
        }
        (KeyCode::Up | KeyCode::Char('k'), _) if result_count > 0 => {
            *selected = if *selected == 0 {
                result_count - 1
            } else {
                *selected - 1
            };
        }
        (KeyCode::Down | KeyCode::Char('j'), _) if result_count > 0 => {
            *selected = (*selected + 1) % result_count;
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        _ => {}
    }
    false
}

fn render_results(
    frame: &mut ratatui::Frame<'_>,
    results: &[TestResult],
    list_state: &mut ListState,
) {
    let area = frame.area();
    let benchmark_mode = results.iter().any(|result| result.benchmark.is_some());
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let title = format!(
        " Solution tester - {}/{} passed ",
        results.iter().filter(|result| result.passed()).count(),
        results.len()
    );
    let stats_text = match (
        benchmark_info(results),
        benchmark_timing_stats(results).or_else(|| timing_stats(results)),
    ) {
        (Some((runs, executions)), Some(stats)) => format!(
            "Benchmark: {runs} runs/test   Executions: {executions}\n\
             Total: {}   Average: {}   P90: {}",
            format_duration(stats.total),
            format_duration(stats.average),
            format_duration(stats.p90)
        ),
        (None, Some(stats)) => format!(
            "Total: {}   Average: {}   P90: {}",
            format_duration(stats.total),
            format_duration(stats.average),
            format_duration(stats.p90)
        ),
        (_, None) => "No tests executed".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(stats_text).style(Style::default().fg(Color::Cyan)),
        sections[0],
    );
    let header = if benchmark_mode {
        format!(
            "  {:<8}{:<42}{:<20}{:<20}{:<20}",
            "RESULT", "PATH", "AVERAGE TIME", "TOTAL", "P90"
        )
    } else {
        format!("  {:<8}{:<42}{:<12}", "RESULT", "PATH", "TIME")
    };
    let items = results
        .iter()
        .map(|result| {
            let color = if result.passed() {
                Color::Green
            } else {
                Color::Red
            };
            let marker = if result.passed() { "PASS" } else { "FAIL" };
            let mut line = vec![
                Span::styled(
                    format!("{marker:<4}    "),
                    Style::default().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!("{:<40}", result.name)),
                Span::raw("  "),
                Span::raw(format!(
                    "{:<20}",
                    format_duration(result.display_duration())
                )),
            ];
            if let Some(benchmark) = &result.benchmark {
                let total = format_duration(benchmark.timing.total);
                let p90 = format_duration(benchmark.timing.p90);
                line.push(Span::styled(
                    format!("{:<20}", total),
                    Style::default().fg(Color::Rgb(255, 165, 0)),
                ));
                line.push(Span::styled(
                    format!("{:<20}", p90),
                    Style::default().fg(Color::Rgb(255, 165, 0)),
                ));
            } else if benchmark_mode {
                line.push(Span::raw("                          "));
            }
            ListItem::new(Line::from(line))
        })
        .collect::<Vec<_>>();

    let table_block = Block::default().borders(Borders::ALL).title(title);
    let table_inner = table_block.inner(sections[2]);
    let table_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(table_inner);
    frame.render_widget(table_block, sections[2]);
    frame.render_widget(
        Paragraph::new(header).style(Style::default().add_modifier(Modifier::BOLD)),
        table_sections[0],
    );
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, table_sections[1], list_state);
    let footer = Paragraph::new("Up/Down or j/k: select   Enter: details   q/Esc: quit")
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, sections[3]);
}

fn render_detail(
    frame: &mut ratatui::Frame<'_>,
    result: Option<&TestResult>,
    scroll: u16,
    benchmark_visible: bool,
) {
    let area = frame.area();
    let Some(result) = result else {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);
        frame.render_widget(
            Paragraph::new("No tests were discovered.\nPress q or Esc to quit.")
                .block(Block::default().borders(Borders::ALL).title(" Details ")),
            sections[0],
        );
        frame.render_widget(
            Paragraph::new("q/Esc: quit").style(Style::default().fg(Color::DarkGray)),
            sections[1],
        );
        return;
    };

    let color = if result.passed() {
        Color::Green
    } else {
        Color::Red
    };
    let summary = vec![
        Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(result.status.to_string(), Style::default().fg(color)),
        ]),
        Line::from(format!(
            "Duration: {}",
            format_duration(result.display_duration())
        )),
        Line::from(format!(
            "Exit code: {}",
            result
                .exit_code
                .map_or_else(|| "none".to_owned(), |code| code.to_string())
        )),
        Line::from(format!("Detail: {}", result.detail)),
    ];
    let mut output_lines = Vec::new();
    output_lines.push(Line::from(Span::styled(
        "Expected output:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    append_output(&mut output_lines, &result.expected);
    output_lines.push(Line::from(""));
    output_lines.push(Line::from(Span::styled(
        "Actual output:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    append_output(&mut output_lines, &result.actual);
    output_lines.push(Line::from(""));
    output_lines.push(Line::from(Span::styled(
        "Stderr:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    append_output(&mut output_lines, &result.stderr);

    let table_height = result
        .benchmark
        .as_ref()
        .filter(|_| benchmark_visible)
        .map_or(0, |benchmark| benchmark.durations.len() as u16 + 7);
    let sections = if table_height > 0 {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Length(table_height),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(6),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(area)
    };
    frame.render_widget(
        Paragraph::new(Text::from(summary)).block(Block::default().borders(Borders::ALL)),
        sections[0],
    );
    let output_index;
    if benchmark_visible {
        if let Some(benchmark) = &result.benchmark {
            let mut rows = benchmark
                .durations
                .iter()
                .enumerate()
                .map(|(index, duration)| {
                    let (difference, difference_color) = benchmark_difference(benchmark, index);
                    Row::new([
                        Cell::from((index + 1).to_string()),
                        Cell::from(format_duration(*duration)),
                        Cell::from(difference).style(Style::default().fg(difference_color)),
                    ])
                })
                .collect::<Vec<_>>();
            rows.extend([
                Row::new(["", "", ""]),
                Row::new([
                    Cell::from("Total"),
                    Cell::from(format_duration(benchmark.timing.total)),
                    Cell::from(""),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
                Row::new([
                    Cell::from("Average"),
                    Cell::from(format_duration(benchmark.timing.average)),
                    Cell::from(""),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
                Row::new([
                    Cell::from("P90"),
                    Cell::from(format_duration(benchmark.timing.p90)),
                    Cell::from(""),
                ])
                .style(Style::default().add_modifier(Modifier::BOLD)),
            ]);
            let table = Table::new(
                rows,
                [
                    Constraint::Length(8),
                    Constraint::Length(12),
                    Constraint::Length(14),
                ],
            )
            .header(
                Row::new(["Run", "Time", "Difference"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" Execution times ({} runs) ", benchmark.runs)),
            );
            frame.render_widget(table, sections[1]);
            output_index = 2;
        } else {
            output_index = 1;
        }
    } else {
        output_index = 1;
    }
    let detail = Paragraph::new(Text::from(output_lines))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", result.name)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(detail, sections[output_index]);
    frame.render_widget(
        Paragraph::new(if result.benchmark.is_some() {
            "Up/Down or j/k: scroll   e: show/hide benchmark   Enter/Esc/q: back"
        } else {
            "Up/Down or j/k: scroll   Enter/Esc/q: back"
        })
        .style(Style::default().fg(Color::DarkGray)),
        sections[sections.len() - 1],
    );
}

fn append_output(lines: &mut Vec<Line<'static>>, output: &str) {
    if output.is_empty() {
        lines.push(Line::from("<empty>"));
    } else {
        lines.extend(output.lines().map(|line| Line::from(line.to_owned())));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn normalizes_trailing_whitespace_and_final_newline() {
        assert_eq!(normalize_output("a  \n b\t\r\n"), "a\n b");
    }

    #[test]
    fn formats_short_durations_in_milliseconds() {
        assert_eq!(format_duration(Duration::from_millis(42)), "42ms");
        assert_eq!(format_duration(Duration::from_millis(999)), "999ms");
        assert_eq!(format_duration(Duration::from_secs(1)), "1.000s");
    }

    #[test]
    fn calculates_total_average_and_nearest_rank_p90() {
        let results = [10, 20, 30, 40, 100]
            .into_iter()
            .map(|milliseconds| TestResult {
                name: String::new(),
                status: TestStatus::Passed,
                duration: Duration::from_millis(milliseconds),
                benchmark: None,
                expected: String::new(),
                actual: String::new(),
                stderr: String::new(),
                exit_code: Some(0),
                detail: String::new(),
            })
            .collect::<Vec<_>>();

        assert_eq!(
            timing_stats(&results),
            Some(TimingStats {
                total: Duration::from_millis(200),
                average: Duration::from_millis(40),
                p90: Duration::from_millis(100),
            })
        );
        assert_eq!(timing_stats(&[]), None);
    }

    #[test]
    fn benchmark_argument_is_opt_in_and_requires_multiple_runs() {
        let normal = Cli::try_parse_from(["solution-tester", "solution", "tests"]).unwrap();
        assert_eq!(normal.benchmark, None);

        let benchmark =
            Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark", "3"])
                .unwrap();
        assert_eq!(benchmark.benchmark, Some(3));

        let default_benchmark =
            Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark"]).unwrap();
        assert_eq!(default_benchmark.benchmark, Some(10));

        assert!(
            Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark", "1"])
                .is_err()
        );
    }

    #[test]
    fn wraps_test_selection_in_both_directions() {
        let mut selected = 0;
        let mut detail = false;
        let mut scroll = 0;
        let mut benchmark_visible = true;

        handle_key(
            KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            3,
            &mut selected,
            &mut detail,
            &mut scroll,
            &mut benchmark_visible,
        );
        assert_eq!(selected, 2);

        handle_key(
            KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
            3,
            &mut selected,
            &mut detail,
            &mut scroll,
            &mut benchmark_visible,
        );
        assert_eq!(selected, 0);
    }

    #[test]
    fn toggles_benchmark_details_with_e() {
        let mut selected = 0;
        let mut detail = true;
        let mut scroll = 4;
        let mut benchmark_visible = true;

        handle_key(
            KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
            1,
            &mut selected,
            &mut detail,
            &mut scroll,
            &mut benchmark_visible,
        );

        assert!(!benchmark_visible);
        assert_eq!(scroll, 0);
    }

    #[test]
    fn calculates_colored_benchmark_differences() {
        let benchmark = BenchmarkStats {
            runs: 3,
            timing: TimingStats {
                total: Duration::from_millis(600),
                average: Duration::from_millis(200),
                p90: Duration::from_millis(300),
            },
            durations: vec![
                Duration::from_millis(200),
                Duration::from_millis(150),
                Duration::from_millis(175),
            ],
        };

        assert_eq!(
            benchmark_difference(&benchmark, 0),
            ("-".to_owned(), Color::DarkGray)
        );
        assert_eq!(
            benchmark_difference(&benchmark, 1),
            ("-50.000ms".to_owned(), Color::Green)
        );
        assert_eq!(
            benchmark_difference(&benchmark, 2),
            ("+25.000ms".to_owned(), Color::Red)
        );
    }

    #[test]
    fn formats_benchmark_differences_with_sub_millisecond_precision() {
        assert_eq!(
            format_benchmark_difference(Duration::from_micros(123)),
            "0.123ms"
        );
        assert_eq!(
            format_benchmark_difference(Duration::from_micros(999_999)),
            "999.999ms"
        );
        assert_eq!(
            format_benchmark_difference(Duration::from_millis(1_234)),
            "1.234s"
        );
    }

    #[test]
    fn reports_first_mismatch_line() {
        assert_eq!(
            mismatch_detail("one\ntwo\n", "one\nthree\n"),
            "first mismatch at line 2"
        );
    }

    #[test]
    fn discovers_recursive_pairs_and_unmatched_files() {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir_all(directory.path().join("nested")).unwrap();
        fs::write(directory.path().join("root.in"), b"1").unwrap();
        fs::write(directory.path().join("root.sol"), b"1").unwrap();
        fs::write(directory.path().join("nested/missing.in"), b"2").unwrap();
        fs::write(directory.path().join("orphan.sol"), b"3").unwrap();

        let tests = discover_tests(directory.path()).unwrap();
        let names: Vec<_> = tests.iter().map(TestCase::display_name).collect();
        assert_eq!(
            names,
            vec![
                "nested/missing".to_owned(),
                "orphan".to_owned(),
                "root".to_owned()
            ]
        );
        assert!(tests[0].expected_path.is_none());
        assert!(tests[1].input_path.is_none());
        assert!(tests[2].input_path.is_some() && tests[2].expected_path.is_some());
    }

    #[cfg(unix)]
    #[test]
    fn executes_a_solution_and_records_duration() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("solution.sh");
        fs::write(&executable, "#!/bin/sh\ncat\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        fs::write(directory.path().join("case.in"), b"hello\n").unwrap();
        fs::write(directory.path().join("case.sol"), b"hello").unwrap();

        let test = discover_tests(directory.path()).unwrap().pop().unwrap();
        let result = run_test(&executable, &test, Duration::from_secs(1));
        assert!(result.passed());
        assert!(!result.duration.is_zero());
    }

    #[cfg(unix)]
    #[test]
    fn classifies_non_zero_exit() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("solution.sh");
        fs::write(&executable, "#!/bin/sh\necho failure >&2\nexit 7\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        fs::write(directory.path().join("case.in"), b"").unwrap();
        fs::write(directory.path().join("case.sol"), b"").unwrap();

        let test = discover_tests(directory.path()).unwrap().pop().unwrap();
        let result = run_test(&executable, &test, Duration::from_secs(1));
        assert_eq!(result.status, TestStatus::NonZeroExit);
        assert_eq!(result.exit_code, Some(7));
        assert!(result.stderr.contains("failure"));
    }

    #[cfg(unix)]
    #[test]
    fn classifies_timeout() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("solution.sh");
        fs::write(&executable, "#!/bin/sh\nexec sleep 1\n").unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        fs::write(directory.path().join("case.in"), b"").unwrap();
        fs::write(directory.path().join("case.sol"), b"").unwrap();

        let test = discover_tests(directory.path()).unwrap().pop().unwrap();
        let result = run_test(&executable, &test, Duration::from_millis(10));
        assert_eq!(result.status, TestStatus::TimedOut);
    }

    #[cfg(unix)]
    #[test]
    fn stops_on_failure_unless_keep_going_is_enabled() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("solution.sh");
        fs::write(
            &executable,
            "#!/bin/sh\nif grep -q fail; then exit 1; else cat; fi\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        fs::write(directory.path().join("a.in"), b"fail\n").unwrap();
        fs::write(directory.path().join("a.sol"), b"unused\n").unwrap();
        fs::write(directory.path().join("b.in"), b"pass\n").unwrap();
        fs::write(directory.path().join("b.sol"), b"pass\n").unwrap();
        let tests = discover_tests(directory.path()).unwrap();

        assert_eq!(
            run_tests(&executable, &tests, Duration::from_secs(1), false, None).len(),
            1
        );
        assert_eq!(
            run_tests(&executable, &tests, Duration::from_secs(1), true, None).len(),
            2
        );
    }

    #[cfg(unix)]
    #[test]
    fn benchmarks_each_test_and_keeps_per_test_timing_stats() {
        let directory = tempfile::tempdir().unwrap();
        let executable = directory.path().join("solution.sh");
        let counter = directory.path().join("count");
        fs::write(&counter, b"0").unwrap();
        fs::write(
            &executable,
            format!(
                "#!/bin/sh\ncount=$(cat '{}')\nprintf '%s' $((count + 1)) > '{}'\ncat\n",
                counter.display(),
                counter.display()
            ),
        )
        .unwrap();
        let mut permissions = fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).unwrap();
        fs::write(directory.path().join("case.in"), b"hello\n").unwrap();
        fs::write(directory.path().join("case.sol"), b"hello").unwrap();

        let tests = discover_tests(directory.path()).unwrap();
        let results = run_tests(&executable, &tests, Duration::from_secs(1), false, Some(3));

        assert_eq!(fs::read_to_string(counter).unwrap(), "3");
        assert_eq!(results.len(), 1);
        let benchmark = results[0].benchmark.as_ref().unwrap();
        assert_eq!(benchmark.runs, 3);
        assert_eq!(benchmark.durations.len(), 3);
        assert!(results[0].passed());
        assert_eq!(benchmark_timing_stats(&results), Some(benchmark.timing));
    }
}
