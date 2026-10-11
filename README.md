# Programming Contests Solution Tester

Run a compiled solution against recursively discovered test cases:

```text
cargo run -- path/to/solution path/to/tests
cargo run -- path/to/solution path/to/tests --keep-going --timeout 10
cargo run -- path/to/solution path/to/tests --keep-going --benchmark 10
```

Each test consists of a matching pair such as `cases/sample.in` and
`cases/sample.sol`. The `.in` file is sent to the executable on standard input;
the executable is run without extra arguments. The expected `.sol` file is
compared with standard output after removing trailing whitespace from lines and
the final newline.

The tester stops after the first failure by default. Use `--keep-going` to run
all discovered pairs. Each test has a 5-second timeout by default; change it
with `--timeout SECONDS`. Missing pair members, unreadable files, spawn errors,
non-zero exits, timeouts, and wrong answers are shown as failed tests.

After execution, the result list shows total runtime, average runtime, and the
nearest-rank p90 runtime across all executed tests. The result list also shows
the input word count for each test. Use Up/Down (or `j`/`k`) to select a result
(selection wraps from the first result to the last and vice versa), Enter to
view expected output, actual output, stderr, exit status, and timing, and `q` or
Escape to quit. Results are green when passed and red when failed. The command
exits with status 1 if any executed test failed.

Use `--benchmark [RUNS]` to run each discovered test case multiple times
(10 runs by default, minimum 2). In benchmark mode, the main page calculates
total, average, and nearest-rank p90 over every execution and displays the
configured runs-per-test and total execution count. Each test's detail view
shows a table containing the duration of every execution. Benchmarking is
disabled unless the option is provided. Press `e` in a benchmark test's detail
view to hide or show the execution table. The table also shows each run's
difference from the previous run, colored green for a faster run and red for a
slower run, followed by total, average, and nearest-rank p90 summary rows. The
main page shows the same total, average, and p90 values beside each benchmark
test's displayed runtime.
