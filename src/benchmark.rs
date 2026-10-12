use crate::{
    domain::{BenchmarkStats, TestCase, TestResult},
    runner::run_test,
};
use ratatui::style::Color;
use std::{path::Path, time::Duration};

pub(crate) fn format_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}

pub(crate) fn format_benchmark_duration(duration: Duration) -> String {
    if duration < Duration::from_secs(1) {
        format!("{:.3}ms", duration.as_secs_f64() * 1000.0)
    } else {
        format!("{:.3}s", duration.as_secs_f64())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TimingStats {
    pub(crate) total: Duration,
    pub(crate) average: Duration,
    pub(crate) p90: Duration,
}

pub(crate) fn timing_stats(results: &[TestResult]) -> Option<TimingStats> {
    timing_stats_for_durations(results.iter().map(|result| result.duration))
}

pub(crate) fn timing_stats_for_durations(
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

pub(crate) fn benchmark_timing_stats(results: &[TestResult]) -> Option<TimingStats> {
    timing_stats_for_durations(
        results
            .iter()
            .filter_map(|result| result.benchmark.as_ref())
            .flat_map(|benchmark| benchmark.durations.iter().copied()),
    )
}

pub(crate) fn benchmark_info(results: &[TestResult]) -> Option<(usize, usize)> {
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

pub(crate) fn format_benchmark_difference(duration: Duration) -> String {
    format_benchmark_duration(duration)
}

pub(crate) fn benchmark_difference(benchmark: &BenchmarkStats, index: usize) -> (String, Color) {
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

pub(crate) fn run_benchmark(
    executable: &Path,
    test: &TestCase,
    timeout: Duration,
    runs: usize,
) -> TestResult {
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
