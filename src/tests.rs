use crate::{
    app::*, benchmark::*, cli::*, comparison::*, discovery::*, domain::*, runner::*, ui::*,
};
use clap::Parser;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::time::Duration;
use std::{fs, sync::mpsc};

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
fn formats_benchmark_milliseconds_with_three_decimal_places() {
    assert_eq!(
        format_benchmark_duration(Duration::from_micros(42_123)),
        "42.123ms"
    );
    assert_eq!(
        format_benchmark_duration(Duration::from_micros(999_999)),
        "999.999ms"
    );
    assert_eq!(format_benchmark_duration(Duration::from_secs(1)), "1.000s");
}

#[test]
fn calculates_total_average_and_nearest_rank_p90() {
    let results = [10, 20, 30, 40, 100]
        .into_iter()
        .map(|milliseconds| TestResult {
            name: String::new(),
            input: String::new(),
            input_words: None,
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
    assert!(normal.async_ui);

    let benchmark =
        Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark", "3"]).unwrap();
    assert_eq!(benchmark.benchmark, Some(3));

    let default_benchmark =
        Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark"]).unwrap();
    assert_eq!(default_benchmark.benchmark, Some(10));

    assert!(
        Cli::try_parse_from(["solution-tester", "solution", "tests", "--benchmark", "1"]).is_err()
    );

    let synchronous =
        Cli::try_parse_from(["solution-tester", "solution", "tests", "--no-async-ui"]).unwrap();
    assert!(!synchronous.async_ui);
}

#[test]
fn keeps_test_selection_within_both_edges() {
    let mut selected = 0;
    let mut detail = false;
    let mut scroll = 0;
    let mut benchmark_visible = true;
    let mut horizontal_scroll = 0;

    handle_key(
        KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(selected, 0);

    handle_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(selected, 1);
}

#[test]
fn moves_main_table_by_column_and_stops_at_the_edges() {
    let mut selected = 0;
    let mut detail = false;
    let mut scroll = 0;
    let mut benchmark_visible = true;
    let mut horizontal_scroll = 0;

    handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 1);

    handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 2);

    handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 2);

    handle_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 1);

    handle_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 0);
}

#[test]
fn control_navigation_jumps_to_main_page_edges() {
    let mut selected = 1;
    let mut detail = false;
    let mut scroll = 0;
    let mut benchmark_visible = true;
    let mut horizontal_scroll = 1;

    handle_key(
        KeyEvent::new(KeyCode::Up, KeyModifiers::CONTROL),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(selected, 0);

    handle_key(
        KeyEvent::new(KeyCode::Down, KeyModifiers::CONTROL),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(selected, 2);

    handle_key(
        KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 0);

    handle_key(
        KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL),
        3,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        2,
    );
    assert_eq!(horizontal_scroll, 2);
}

#[test]
fn calculates_main_table_scroll_positions_from_column_boundaries() {
    assert_eq!(main_table_scroll_positions(200, false), vec![0]);
    assert_eq!(main_table_scroll_positions(40, false), vec![0, 10, 52, 56]);
}

#[test]
fn toggles_benchmark_details_with_e() {
    let mut selected = 0;
    let mut detail = true;
    let mut scroll = 4;
    let mut benchmark_visible = true;
    let mut horizontal_scroll = 0;

    handle_key(
        KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE),
        1,
        &mut selected,
        &mut detail,
        &mut scroll,
        &mut benchmark_visible,
        &mut horizontal_scroll,
        0,
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
fn publishes_each_completed_test_in_order() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("solution.sh");
    fs::write(&executable, "#!/bin/sh\ncat\n").unwrap();
    let mut permissions = fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&executable, permissions).unwrap();
    for name in ["first", "second"] {
        fs::write(directory.path().join(format!("{name}.in")), name).unwrap();
        fs::write(directory.path().join(format!("{name}.sol")), name).unwrap();
    }

    let tests = discover_tests(directory.path()).unwrap();
    let (updates, receiver) = mpsc::channel();
    let results = run_tests_with_updates(
        &executable,
        &tests,
        Duration::from_secs(1),
        true,
        None,
        updates,
    );
    let published = receiver
        .try_iter()
        .filter_map(|update| match update {
            TestUpdate::Result { index, result } => Some((index, *result)),
            TestUpdate::Finished => None,
        })
        .collect::<Vec<_>>();

    assert_eq!(results.len(), 2);
    assert_eq!(
        published
            .iter()
            .map(|(index, _)| *index)
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(published.iter().all(|(_, result)| result.passed()));
}

#[test]
fn resolves_pending_rows_when_execution_finishes() {
    let (updates, receiver) = mpsc::channel();
    let mut completed = TestResult::pending("first".to_owned());
    completed.status = TestStatus::Passed;
    updates
        .send(TestUpdate::Result {
            index: 0,
            result: Box::new(completed),
        })
        .unwrap();
    updates.send(TestUpdate::Finished).unwrap();

    let mut results = vec![
        TestResult::pending("first".to_owned()),
        TestResult::pending("second".to_owned()),
    ];
    apply_updates(&receiver, &mut results);

    assert_eq!(results[0].status, TestStatus::Passed);
    assert_eq!(results[1].status, TestStatus::Skipped);
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
    let (updates, receiver) = mpsc::channel();
    let results = run_tests_with_updates(
        &executable,
        &tests,
        Duration::from_secs(1),
        false,
        Some(3),
        updates,
    );

    assert_eq!(fs::read_to_string(counter).unwrap(), "3");
    assert_eq!(results.len(), 1);
    let benchmark = results[0].benchmark.as_ref().unwrap();
    assert_eq!(benchmark.runs, 3);
    assert_eq!(benchmark.durations.len(), 3);
    assert!(results[0].passed());
    assert_eq!(benchmark_timing_stats(&results), Some(benchmark.timing));
    let updates = receiver.try_iter().collect::<Vec<_>>();
    assert!(matches!(updates.last(), Some(TestUpdate::Finished)));
    assert!(matches!(
        updates.first(),
        Some(TestUpdate::Result { result, .. }) if result.benchmark.as_ref().is_some_and(|benchmark| benchmark.runs == 3)
    ));
}
