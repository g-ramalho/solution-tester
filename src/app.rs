use crate::{
    benchmark,
    cli::Cli,
    discovery,
    domain::{TestCase, TestResult},
    runner, ui,
};
use anyhow::{Result, bail};
use std::path::Path;
use std::time::Duration;

pub(crate) fn run(cli: Cli) -> Result<()> {
    validate_paths(&cli)?;
    let tests = discovery::discover_tests(&cli.directory)?;
    let results = run_tests(
        &cli.executable,
        &tests,
        Duration::from_secs(cli.timeout),
        cli.keep_going,
        cli.benchmark,
    );
    ui::run_results_ui(&results)?;
    if results.iter().any(|result| !result.passed()) {
        std::process::exit(1);
    }
    Ok(())
}

pub(crate) fn run_tests(
    executable: &Path,
    tests: &[TestCase],
    timeout: Duration,
    keep_going: bool,
    benchmark_runs: Option<usize>,
) -> Vec<TestResult> {
    let mut results = Vec::with_capacity(tests.len());
    for test in tests {
        let result = benchmark_runs.map_or_else(
            || runner::run_test(executable, test, timeout),
            |runs| benchmark::run_benchmark(executable, test, timeout, runs),
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
