use std::cmp::min;

pub(crate) fn display_bytes(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

pub(crate) fn word_count(bytes: &[u8]) -> usize {
    display_bytes(bytes).split_whitespace().count()
}

pub(crate) fn format_word_count(count: Option<usize>) -> String {
    count.map_or_else(|| "-".to_owned(), |count| count.to_string())
}

pub(crate) fn normalize_output(output: &str) -> String {
    output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn mismatch_detail(expected: &str, actual: &str) -> String {
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
