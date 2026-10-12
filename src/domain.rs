use crate::benchmark::TimingStats;
use std::{fmt::Display, path::PathBuf, time::Duration};
#[derive(Debug, Clone)]
pub(crate) struct TestCase {
    pub(crate) key: PathBuf,
    pub(crate) input_path: Option<PathBuf>,
    pub(crate) expected_path: Option<PathBuf>,
}

impl TestCase {
    pub(crate) fn display_name(&self) -> String {
        self.key.to_string_lossy().into_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TestStatus {
    Pending,
    Skipped,
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
            Self::Pending => "pending",
            Self::Skipped => "not run",
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
pub(crate) struct TestResult {
    pub(crate) name: String,
    pub(crate) input: String,
    pub(crate) input_words: Option<usize>,
    pub(crate) status: TestStatus,
    pub(crate) duration: Duration,
    pub(crate) benchmark: Option<BenchmarkStats>,
    pub(crate) expected: String,
    pub(crate) actual: String,
    pub(crate) stderr: String,
    pub(crate) exit_code: Option<i32>,
    pub(crate) detail: String,
}

impl TestResult {
    pub(crate) fn pending(name: String) -> Self {
        Self {
            name,
            input: String::new(),
            input_words: None,
            status: TestStatus::Pending,
            duration: Duration::ZERO,
            benchmark: None,
            expected: String::new(),
            actual: String::new(),
            stderr: String::new(),
            exit_code: None,
            detail: "test has not started".to_owned(),
        }
    }

    pub(crate) fn skipped(name: String) -> Self {
        Self {
            name,
            input: String::new(),
            input_words: None,
            status: TestStatus::Skipped,
            duration: Duration::ZERO,
            benchmark: None,
            expected: String::new(),
            actual: String::new(),
            stderr: String::new(),
            exit_code: None,
            detail: "test was not run after an earlier failure".to_owned(),
        }
    }

    pub(crate) fn passed(&self) -> bool {
        self.status.is_passed()
    }

    pub(crate) fn is_pending(&self) -> bool {
        matches!(self.status, TestStatus::Pending)
    }

    pub(crate) fn is_skipped(&self) -> bool {
        matches!(self.status, TestStatus::Skipped)
    }

    pub(crate) fn display_duration(&self) -> Duration {
        self.benchmark
            .as_ref()
            .map_or(self.duration, |benchmark| benchmark.timing.average)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BenchmarkStats {
    pub(crate) runs: usize,
    pub(crate) timing: TimingStats,
    pub(crate) durations: Vec<Duration>,
}
