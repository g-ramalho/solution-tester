use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
pub(crate) fn parse_benchmark_runs(value: &str) -> Result<usize, String> {
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
pub(crate) struct Cli {
    /// Executable to test.
    pub(crate) executable: PathBuf,

    /// Directory containing test files.
    pub(crate) directory: PathBuf,

    /// Continue running tests after a failure.
    #[arg(long)]
    pub(crate) keep_going: bool,

    /// Maximum runtime for each test, in seconds.
    #[arg(long, default_value_t = 5, value_parser = clap::value_parser!(u64).range(1..))]
    pub(crate) timeout: u64,

    /// Run every test multiple times and report benchmark timing statistics.
    #[arg(
        long,
        value_name = "RUNS",
        num_args = 0..=1,
        default_missing_value = "10",
        value_parser = parse_benchmark_runs
    )]
    pub(crate) benchmark: Option<usize>,
}
