use crate::{
    app::TestUpdate,
    benchmark::{benchmark_difference, benchmark_info, benchmark_timing_stats, timing_stats},
    benchmark::{format_benchmark_duration, format_duration},
    comparison::format_word_count,
    domain::{TestCase, TestResult},
};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Paragraph, Row, Table, Tabs, Wrap},
};
use std::{io, sync::mpsc::Receiver, time::Duration};

pub(crate) fn run_results_ui(
    tests: &[TestCase],
    updates: Receiver<TestUpdate>,
    benchmark_runs: Option<usize>,
) -> Result<()> {
    enable_raw_mode().context("enable raw terminal mode")?;
    let mut stdout = io::stdout();
    if let Err(error) = execute!(stdout, EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(error).context("enter alternate screen");
    }

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    let ui_result = run_ui_loop(&mut terminal, tests, &updates, benchmark_runs);
    let disable_result = disable_raw_mode().context("restore terminal mode");
    let leave_result =
        execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen");
    let cursor_result = terminal.show_cursor().context("show terminal cursor");

    ui_result?;
    disable_result?;
    leave_result?;
    cursor_result
}

pub(crate) fn run_ui_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    tests: &[TestCase],
    updates: &Receiver<TestUpdate>,
    benchmark_runs: Option<usize>,
) -> Result<()> {
    let mut results = tests
        .iter()
        .map(|test| TestResult::pending(test.display_name()))
        .collect::<Vec<_>>();
    let mut selected = 0;
    let mut detail = false;
    let mut detail_tab = DetailTab::Output;
    let mut detail_scroll = 0;
    let mut benchmark_visible = true;
    let mut horizontal_scroll = 0;
    let mut list_state = ListState::default();
    list_state.select((!results.is_empty()).then_some(selected));

    loop {
        apply_updates(updates, &mut results);
        let benchmark_mode = benchmark_runs.is_some();
        let terminal_width = terminal.size()?.width;
        let scroll_positions = main_table_scroll_positions(terminal_width, benchmark_mode);
        let horizontal_scroll_max = scroll_positions.len().saturating_sub(1);
        terminal.draw(|frame| {
            if detail {
                render_detail(
                    frame,
                    results.get(selected),
                    detail_scroll,
                    benchmark_visible,
                    detail_tab,
                );
            } else {
                render_results(
                    frame,
                    &results,
                    &mut list_state,
                    horizontal_scroll,
                    benchmark_mode,
                    benchmark_runs,
                );
            }
        })?;

        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };

        if handle_key_with_tab(
            key,
            results.len(),
            &mut selected,
            &mut detail,
            &mut detail_scroll,
            &mut detail_tab,
            &mut benchmark_visible,
            &mut horizontal_scroll,
            horizontal_scroll_max,
        ) {
            break;
        }

        list_state.select((!results.is_empty()).then_some(selected));
    }
    Ok(())
}

pub(crate) fn apply_updates(updates: &Receiver<TestUpdate>, results: &mut [TestResult]) {
    for update in updates.try_iter() {
        match update {
            TestUpdate::Result { index, result } => {
                if let Some(row) = results.get_mut(index) {
                    *row = *result;
                }
            }
            TestUpdate::Finished => {
                for result in results.iter_mut().filter(|result| result.is_pending()) {
                    *result = TestResult::skipped(result.name.clone());
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) fn handle_key(
    key: KeyEvent,
    result_count: usize,
    selected: &mut usize,
    detail: &mut bool,
    detail_scroll: &mut u16,
    benchmark_visible: &mut bool,
    horizontal_scroll: &mut usize,
    horizontal_scroll_max: usize,
) -> bool {
    let mut detail_tab = DetailTab::Output;
    handle_key_with_tab(
        key,
        result_count,
        selected,
        detail,
        detail_scroll,
        &mut detail_tab,
        benchmark_visible,
        horizontal_scroll,
        horizontal_scroll_max,
    )
}

pub(crate) fn handle_key_with_tab(
    key: KeyEvent,
    result_count: usize,
    selected: &mut usize,
    detail: &mut bool,
    detail_scroll: &mut u16,
    detail_tab: &mut DetailTab,
    benchmark_visible: &mut bool,
    horizontal_scroll: &mut usize,
    horizontal_scroll_max: usize,
) -> bool {
    match (key.code, key.modifiers) {
        (KeyCode::Char('q'), _) | (KeyCode::Esc, _) => {
            if *detail {
                *detail = false;
                *detail_scroll = 0;
                *detail_tab = DetailTab::Output;
            } else {
                return true;
            }
        }
        (KeyCode::Enter, _) if result_count > 0 => {
            *detail = !*detail;
            *detail_scroll = 0;
            *detail_tab = DetailTab::Output;
        }
        (KeyCode::Tab, _) if *detail => {
            *detail_tab = detail_tab.next();
            *detail_scroll = 0;
        }
        (KeyCode::Left, _) if *detail => {
            *detail_tab = DetailTab::Output;
            *detail_scroll = 0;
        }
        (KeyCode::Right, _) if *detail => {
            *detail_tab = DetailTab::Input;
            *detail_scroll = 0;
        }
        (KeyCode::Char('e'), _) if *detail => {
            *benchmark_visible = !*benchmark_visible;
            *detail_scroll = 0;
        }
        (KeyCode::Up, KeyModifiers::CONTROL) if !*detail && result_count > 0 => {
            *selected = 0;
        }
        (KeyCode::Down, KeyModifiers::CONTROL) if !*detail && result_count > 0 => {
            *selected = result_count - 1;
        }
        (KeyCode::Left, KeyModifiers::CONTROL) if !*detail => {
            *horizontal_scroll = 0;
        }
        (KeyCode::Right, KeyModifiers::CONTROL) if !*detail => {
            *horizontal_scroll = horizontal_scroll_max;
        }
        (KeyCode::Up | KeyCode::Char('k'), _) if *detail => {
            *detail_scroll = detail_scroll.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), _) if *detail => {
            *detail_scroll = detail_scroll.saturating_add(1);
        }
        (KeyCode::Up | KeyCode::Char('k'), _) if result_count > 0 => {
            *selected = selected.saturating_sub(1);
        }
        (KeyCode::Down | KeyCode::Char('j'), _) if result_count > 0 => {
            *selected = (*selected + 1).min(result_count - 1);
        }
        (KeyCode::Left, _) if !*detail => {
            *horizontal_scroll = horizontal_scroll.saturating_sub(1);
        }
        (KeyCode::Right, _) if !*detail => {
            *horizontal_scroll = (*horizontal_scroll + 1).min(horizontal_scroll_max);
        }
        (KeyCode::Char('c'), KeyModifiers::CONTROL) => return true,
        _ => {}
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Output,
    Input,
}

impl DetailTab {
    fn next(self) -> Self {
        match self {
            Self::Output => Self::Input,
            Self::Input => Self::Output,
        }
    }
}

pub(crate) fn render_results(
    frame: &mut ratatui::Frame<'_>,
    results: &[TestResult],
    list_state: &mut ListState,
    horizontal_scroll: usize,
    benchmark_mode: bool,
    benchmark_runs: Option<usize>,
) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    let completed = results.iter().filter(|result| !result.is_pending()).count();
    let title = format!(
        " Solution tester - {}/{} passed ({completed}/{} complete) ",
        results.iter().filter(|result| result.passed()).count(),
        results.len(),
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
        (_, None) => benchmark_runs.map_or_else(
            || "No tests completed yet".to_owned(),
            |runs| format!("Benchmark: {runs} runs/test   No tests completed yet"),
        ),
    };
    frame.render_widget(
        Paragraph::new(stats_text).style(Style::default().fg(Color::Cyan)),
        sections[0],
    );
    let column_widths = main_table_column_widths(benchmark_mode);
    let scroll_positions = main_table_scroll_positions(area.width, benchmark_mode);
    let scroll_position = scroll_positions[horizontal_scroll.min(scroll_positions.len() - 1)];
    let items = results
        .iter()
        .map(|result| {
            let (color, marker) = if result.is_pending() {
                (Color::Yellow, "WAIT")
            } else if result.is_skipped() {
                (Color::DarkGray, "SKIP")
            } else if result.passed() {
                (Color::Green, "PASS")
            } else {
                (Color::Red, "FAIL")
            };
            let mut line = vec![Span::styled(
                format!("{:<10}", format!("{marker:<4}    ")),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )];
            line.push(Span::raw(format!(
                "{:<42}",
                fit_table_cell(&result.name, column_widths[1])
            )));
            line.push(Span::raw(format!(
                "{:<20}",
                format_word_count(result.input_words)
            )));
            line.push(Span::raw(format!(
                "{:<22}",
                if result.is_pending() || result.is_skipped() {
                    "-".to_owned()
                } else if benchmark_mode {
                    format_benchmark_duration(result.display_duration())
                } else {
                    format_duration(result.display_duration())
                }
            )));
            if let Some(benchmark) = &result.benchmark {
                line.push(Span::styled(
                    format!("{:<20}", format_benchmark_duration(benchmark.timing.total)),
                    Style::default().fg(Color::Rgb(255, 165, 0)),
                ));
                line.push(Span::styled(
                    format!("{:<20}", format_benchmark_duration(benchmark.timing.p90)),
                    Style::default().fg(Color::Rgb(255, 165, 0)),
                ));
            } else if benchmark_mode {
                line.extend([
                    Span::raw("                    "),
                    Span::raw("                    "),
                ]);
            }
            ListItem::new(crop_line(
                Line::from(line),
                scroll_position,
                table_sections_width(area.width),
            ))
        })
        .collect::<Vec<_>>();

    let header_cells = if benchmark_mode {
        [
            "RESULT",
            "PATH",
            "INPUT WORD COUNT",
            "AVERAGE",
            "TOTAL",
            "P90",
        ]
        .into_iter()
        .collect::<Vec<_>>()
    } else {
        ["RESULT", "PATH", "INPUT WORD COUNT", "TIME"]
            .into_iter()
            .collect::<Vec<_>>()
    };
    let header = crop_line(
        Line::from(
            std::iter::once(Span::raw("  "))
                .chain(
                    header_cells
                        .into_iter()
                        .zip(column_widths.iter().copied())
                        .map(|(cell, width)| Span::raw(format!("{cell:<width$}"))),
                )
                .collect::<Vec<_>>(),
        )
        .style(Style::default().add_modifier(Modifier::BOLD)),
        scroll_position,
        table_sections_width(area.width),
    );

    let table_block = Block::default().borders(Borders::ALL).title(title);
    let table_inner = table_block.inner(sections[2]);
    let table_sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(1)])
        .split(table_inner);
    let has_left = scroll_position > 0;
    let has_right =
        scroll_position + table_sections_width(area.width) < main_table_width(benchmark_mode);
    frame.render_widget(table_block, sections[2]);
    frame.render_widget(Paragraph::new(header), table_sections[0]);
    let arrow_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    if has_left {
        frame.render_widget(
            Paragraph::new("←").style(arrow_style),
            Rect::new(table_sections[0].x, table_sections[0].y, 1, 1),
        );
    }
    if has_right && table_sections[0].width > 1 {
        frame.render_widget(
            Paragraph::new("→").style(arrow_style),
            Rect::new(
                table_sections[0].x + table_sections[0].width - 1,
                table_sections[0].y,
                1,
                1,
            ),
        );
    }
    let list = List::new(items)
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    frame.render_stateful_widget(list, table_sections[1], list_state);
    let footer = Paragraph::new(
        "←/→: scroll columns (Ctrl: first/last)   ↑/↓ or j/k: select (Ctrl: first/last)   Enter: details   q/Esc: quit",
    )
    .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(footer, sections[3]);
}

pub(crate) fn main_table_column_widths(benchmark_mode: bool) -> Vec<usize> {
    if benchmark_mode {
        vec![10, 42, 20, 22, 20, 20]
    } else {
        vec![10, 42, 20, 22]
    }
}

pub(crate) fn main_table_width(benchmark_mode: bool) -> usize {
    main_table_column_widths(benchmark_mode).into_iter().sum()
}

pub(crate) fn table_sections_width(area_width: u16) -> usize {
    area_width.saturating_sub(2) as usize
}

pub(crate) fn main_table_scroll_positions(area_width: u16, benchmark_mode: bool) -> Vec<usize> {
    let viewport_width = table_sections_width(area_width);
    let table_width = main_table_width(benchmark_mode);
    let max_scroll = table_width.saturating_sub(viewport_width);
    if max_scroll == 0 {
        return vec![0];
    }

    let mut positions = vec![0];
    let mut position = 0;
    for width in main_table_column_widths(benchmark_mode) {
        if position >= max_scroll {
            break;
        }
        position += width;
        if position < max_scroll {
            positions.push(position);
        }
    }
    if positions.last().copied() != Some(max_scroll) {
        positions.push(max_scroll);
    }
    positions
}

pub(crate) fn fit_table_cell(value: &str, width: usize) -> String {
    value.chars().take(width).collect()
}

pub(crate) fn crop_line(line: Line<'static>, offset: usize, width: usize) -> Line<'static> {
    let mut remaining = width;
    let mut skipped = offset;
    let mut spans = Vec::new();
    for span in line.spans {
        if remaining == 0 {
            break;
        }
        let content = span.content.into_owned();
        let chars = content.chars().collect::<Vec<_>>();
        if skipped >= chars.len() {
            skipped -= chars.len();
            continue;
        }
        let start = skipped;
        skipped = 0;
        let end = (start + remaining).min(chars.len());
        spans.push(Span::styled(
            chars[start..end].iter().collect::<String>(),
            span.style,
        ));
        remaining -= end - start;
    }
    Line::from(spans)
}

pub(crate) fn render_detail(
    frame: &mut ratatui::Frame<'_>,
    result: Option<&TestResult>,
    scroll: u16,
    benchmark_visible: bool,
    tab: DetailTab,
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

    let color = if result.is_pending() {
        Color::Yellow
    } else if result.is_skipped() {
        Color::DarkGray
    } else if result.passed() {
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
    let mut content_lines = Vec::new();
    match tab {
        DetailTab::Output => {
            content_lines.push(Line::from(Span::styled(
                "Expected output:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            append_output(&mut content_lines, &result.expected);
            content_lines.push(Line::from(""));
            content_lines.push(Line::from(Span::styled(
                "Actual output:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            append_output(&mut content_lines, &result.actual);
            content_lines.push(Line::from(""));
            content_lines.push(Line::from(Span::styled(
                "Stderr:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            append_output(&mut content_lines, &result.stderr);
        }
        DetailTab::Input => append_output(&mut content_lines, &result.input),
    }

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
    let content_index;
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
            content_index = 2;
        } else {
            content_index = 1;
        }
    } else {
        content_index = 1;
    }
    let content_area = sections[content_index];
    let detail = Paragraph::new(Text::from(content_lines))
        .block(Block::default().borders(Borders::ALL))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    frame.render_widget(detail, content_area);
    let tabs = Tabs::new(vec!["Output", "Input"])
        .select(if tab == DetailTab::Input { 1 } else { 0 })
        .highlight_style(
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )
        .style(Style::default().fg(Color::White))
        .divider(symbols::DOT)
        .padding(" ", " ");
    frame.render_widget(
        tabs,
        Rect::new(
            content_area.x.saturating_add(1),
            content_area.y,
            content_area.width.saturating_sub(2),
            1,
        ),
    );
    frame.render_widget(
        Paragraph::new(if result.benchmark.is_some() {
            "Up/Down or j/k: scroll   Tab/←/→: switch tab   e: show/hide benchmark   Enter/Esc/q: back"
        } else {
            "Up/Down or j/k: scroll   Tab/←/→: switch tab   Enter/Esc/q: back"
        })
        .style(Style::default().fg(Color::DarkGray)),
        sections[sections.len() - 1],
    );
}

pub(crate) fn append_output(lines: &mut Vec<Line<'static>>, output: &str) {
    if output.is_empty() {
        lines.push(Line::from("<empty>"));
    } else {
        lines.extend(output.lines().map(|line| Line::from(line.to_owned())));
    }
}
