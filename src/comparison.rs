use std::cmp::min;

pub(crate) fn display_bytes(bytes: &[u8]) -> &str {
    std::str::from_utf8(bytes).unwrap_or_default()
}

pub(crate) fn display_trimmed(bytes: &[u8]) -> &str {
    display_bytes(bytes).trim()
}

pub(crate) fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
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

pub(crate) fn outputs_match(expected: &str, actual: &str) -> bool {
    normalize_output(expected.trim()) == normalize_output(actual.trim())
}

pub(crate) fn mismatch_detail(expected: &str, actual: &str) -> String {
    let normalized_expected = normalize_output(expected.trim());
    let normalized_actual = normalize_output(actual.trim());
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
