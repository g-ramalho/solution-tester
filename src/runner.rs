use crate::{
    comparison::{display_bytes, mismatch_detail, normalize_output, word_count},
    domain::{TestCase, TestResult, TestStatus},
};
use std::{
    fs,
    io::{self, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

pub(crate) fn run_test(executable: &Path, test: &TestCase, timeout: Duration) -> TestResult {
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
    let expected_bytes = match fs::read(expected_path) {
        Ok(expected) => expected,
        Err(error) => {
            return failure_result(
                name,
                TestStatus::ReadError,
                started.elapsed(),
                format!("could not read {}: {error}", expected_path.display()),
            );
        }
    };
    let input_words = Some(word_count(&input));
    let input_display = display_bytes(&input);
    let expected = display_bytes(&expected_bytes);

    let mut child = match Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return failure_result_with_input_words(
                name,
                TestStatus::SpawnError,
                started.elapsed(),
                format!("could not start {}: {error}", executable.display()),
                input_words,
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
        return failure_result_with_input_words(
            name,
            TestStatus::WriteError,
            started.elapsed(),
            format!("could not write test input: {error}"),
            input_words,
        );
    }

    let timed_out = wait_for_child(&mut child, timeout);
    let actual_bytes = join_reader(stdout_reader);
    let stderr_bytes = join_reader(stderr_reader);
    let (actual_bytes, stderr_bytes) = match (actual_bytes, stderr_bytes) {
        (Ok(actual), Ok(stderr)) => (actual, stderr),
        (Err(error), _) | (_, Err(error)) => {
            return failure_result_with_input_words(
                name,
                TestStatus::ReadError,
                started.elapsed(),
                format!("could not read process output: {error}"),
                input_words,
            );
        }
    };
    let actual = display_bytes(&actual_bytes);
    let stderr = display_bytes(&stderr_bytes);
    let duration = started.elapsed();

    if timed_out {
        return TestResult {
            name,
            input: input_display,
            input_words,
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
            input: input_display,
            input_words,
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
            input: input_display,
            input_words,
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
            input: input_display,
            input_words,
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

pub(crate) fn wait_for_child(child: &mut Child, timeout: Duration) -> bool {
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

pub(crate) fn start_reader(
    mut output: impl Read + Send + 'static,
) -> JoinHandle<io::Result<Vec<u8>>> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        output.read_to_end(&mut bytes)?;
        Ok(bytes)
    })
}

pub(crate) fn join_reader(reader: Option<JoinHandle<io::Result<Vec<u8>>>>) -> io::Result<Vec<u8>> {
    let reader = reader.ok_or_else(|| io::Error::other("missing process output pipe"))?;
    reader
        .join()
        .map_err(|_| io::Error::other("process output reader panicked"))?
}

pub(crate) fn failure_result(
    name: String,
    status: TestStatus,
    duration: Duration,
    detail: impl Into<String>,
) -> TestResult {
    failure_result_with_input_words(name, status, duration, detail, None)
}

pub(crate) fn failure_result_with_input_words(
    name: String,
    status: TestStatus,
    duration: Duration,
    detail: impl Into<String>,
    input_words: Option<usize>,
) -> TestResult {
    TestResult {
        name,
        input: String::new(),
        input_words,
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
