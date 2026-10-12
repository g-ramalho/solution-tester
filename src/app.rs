use crate::{
    benchmark,
    cli::Cli,
    discovery,
    domain::{TestCase, TestResult},
    runner, ui,
};
use anyhow::{Result, bail};
use std::{path::Path, sync::mpsc::Sender, thread, time::Duration};

pub(crate) enum TestUpdate {
    Result {
        index: usize,
        result: Box<TestResult>,
    },
    Finished,
}

pub(crate) fn run(cli: Cli) -> Result<()> {
    validate_paths(&cli)?;
    let tests = discovery::discover_tests(&cli.directory)?;
    if !cli.async_ui {
        let results = run_tests(
            &cli.executable,
            &tests,
            Duration::from_secs(cli.timeout),
            cli.keep_going,
            cli.benchmark,
        );
        let (updates, update_receiver) = std::sync::mpsc::channel();
        for (index, result) in results.iter().cloned().enumerate() {
            updates
                .send(TestUpdate::Result {
                    index,
                    result: Box::new(result),
                })
                .map_err(|_| anyhow::anyhow!("could not initialize result UI"))?;
        }
        updates
            .send(TestUpdate::Finished)
            .map_err(|_| anyhow::anyhow!("could not initialize result UI"))?;
        ui::run_results_ui(&tests, update_receiver, cli.benchmark)?;
        return finish(results);
    }

    let (updates, update_receiver) = std::sync::mpsc::channel();
    let executable = cli.executable.clone();
    let worker_tests = tests.clone();
    let timeout = Duration::from_secs(cli.timeout);
    let keep_going = cli.keep_going;
    let benchmark_runs = cli.benchmark;
    let worker = thread::spawn(move || {
        run_tests_with_updates(
            &executable,
            &worker_tests,
            timeout,
            keep_going,
            benchmark_runs,
            updates,
        )
    });
    let ui_result = ui::run_results_ui(&tests, update_receiver, cli.benchmark);
    let results = worker
        .join()
        .map_err(|_| anyhow::anyhow!("test worker thread panicked"))?;
    ui_result?;
    finish(results)
}

fn finish(results: Vec<TestResult>) -> Result<()> {
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
    run_tests_internal(
        executable,
        tests,
        timeout,
        keep_going,
        benchmark_runs,
        |_, _| true,
    )
}

pub(crate) fn run_tests_with_updates(
    executable: &Path,
    tests: &[TestCase],
    timeout: Duration,
    keep_going: bool,
    benchmark_runs: Option<usize>,
    updates: Sender<TestUpdate>,
) -> Vec<TestResult> {
    let results = run_tests_internal(
        executable,
        tests,
        timeout,
        keep_going,
        benchmark_runs,
        |index, result| {
            updates
                .send(TestUpdate::Result {
                    index,
                    result: Box::new(result),
                })
                .is_ok()
        },
    );
    let _ = updates.send(TestUpdate::Finished);
    results
}

fn run_tests_internal(
    executable: &Path,
    tests: &[TestCase],
    timeout: Duration,
    keep_going: bool,
    benchmark_runs: Option<usize>,
    mut publish: impl FnMut(usize, TestResult) -> bool,
) -> Vec<TestResult> {
    let mut results = Vec::with_capacity(tests.len());
    for (index, test) in tests.iter().enumerate() {
        let result = benchmark_runs.map_or_else(
            || runner::run_test(executable, test, timeout),
            |runs| benchmark::run_benchmark(executable, test, timeout, runs),
        );
        let failed = !result.passed();
        results.push(result.clone());
        if !publish(index, result) {
            break;
        }
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
