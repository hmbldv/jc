//! Shared literal escaping for Atlassian query languages.
//!
//! JQL and CQL share the same string literal grammar: double-quoted with
//! backslash escapes for `\` and `"`. Centralizing the helper here means
//! both product crates produce identically-safe queries.

/// Escape a user-supplied string for use as a JQL or CQL literal. Wraps in
/// double quotes and backslash-escapes `\` and `"`. Order matters: escape
/// backslash first so the quote escape's own backslash isn't double-escaped.
pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

/// Validate a JQL/CQL relative time expression like `-7d`, `+2w`, `-24h`.
///
/// Grammar: `[+-]?[0-9]+[smhdwMy]`
/// - `s` seconds, `m` minutes, `h` hours, `d` days, `w` weeks, `M` months, `y` years
///
/// Rejects everything else so that user input cannot inject arbitrary JQL
/// through a raw time clause. Complex time expressions should go through
/// the `jql` / `cql` raw escape hatch commands instead.
pub fn is_valid_relative_time(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    let mut i = 0;
    if matches!(bytes[0], b'+' | b'-') {
        i += 1;
    }
    let digits_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == digits_start {
        return false;
    }
    if i != bytes.len() - 1 {
        return false;
    }
    matches!(
        bytes[i],
        b's' | b'm' | b'h' | b'd' | b'w' | b'M' | b'y'
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_plain() {
        assert_eq!(escape_string("hello"), "\"hello\"");
    }

    #[test]
    fn escape_quote() {
        assert_eq!(escape_string(r#"he said "hi""#), r#""he said \"hi\"""#);
    }

    #[test]
    fn escape_backslash() {
        assert_eq!(escape_string(r"path\to\thing"), r#""path\\to\\thing""#);
    }

    #[test]
    fn escape_backslash_before_quote() {
        // `\"` in input must NOT collapse to a single escaped-quote; it must
        // become `\\\"` — escaped backslash followed by escaped quote.
        assert_eq!(escape_string("\\\""), r#""\\\"""#);
    }

    #[test]
    fn time_relative_valid() {
        assert!(is_valid_relative_time("-7d"));
        assert!(is_valid_relative_time("+2w"));
        assert!(is_valid_relative_time("24h"));
        assert!(is_valid_relative_time("-1m"));
        assert!(is_valid_relative_time("-6M"));
        assert!(is_valid_relative_time("1y"));
    }

    #[test]
    fn time_relative_rejects_injection() {
        assert!(!is_valid_relative_time("-7d OR project = SECRET"));
        assert!(!is_valid_relative_time("-7d\""));
        assert!(!is_valid_relative_time("-7"));
        assert!(!is_valid_relative_time("d"));
        assert!(!is_valid_relative_time(""));
        assert!(!is_valid_relative_time("--7d"));
        assert!(!is_valid_relative_time("7 d"));
        assert!(!is_valid_relative_time("7days"));
    }
}
