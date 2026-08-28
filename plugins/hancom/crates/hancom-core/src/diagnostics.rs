//! Sanitization at terminal and single-line log boundaries.

use std::path::Path;

/// Convert untrusted text to a terminal- and single-line-log-safe form.
pub fn escape_diagnostic_text(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_control() || matches!(ch, '\u{2028}' | '\u{2029}') {
            escaped.extend(ch.escape_default());
        } else {
            escaped.push(ch);
        }
    }
    escaped
}

/// Render an untrusted path through the same diagnostic boundary.
pub fn diagnostic_path(path: &Path) -> String {
    escape_diagnostic_text(&path.to_string_lossy())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn printable_unicode_is_preserved() {
        assert_eq!(escape_diagnostic_text("한컴 문서"), "한컴 문서");
    }
}
