//! Control-character sanitization for TTY output.
//!
//! Server-sourced strings (issue summaries, comment bodies, page titles,
//! diff output) are printed to stderr in `--confirm` and `--verbose` modes.
//! A malicious actor who can set a Jira/Confluence summary can stuff it
//! with ANSI escape sequences that rewrite the terminal, clear the screen,
//! pre-position the cursor over a confirmation prompt, or (in some terminals)
//! inject keystrokes via DCS/OSC.
//!
//! The rule: anything that might be server- or user-influenced and lands on
//! a TTY goes through [`sanitize`] first. JSON output on stdout is already
//! safe because `serde_json` escapes control characters in string values.

/// Strip C0/C1 control characters (except `\n` and `\t`) and DEL (`\x7f`),
/// replacing each with U+FFFD. Preserves newlines and tabs so diffs and
/// multi-line previews still render legibly.
pub fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\n' | '\t' => c,
            // C0 and C1 control ranges, plus DEL
            c if (c as u32) < 0x20 || c == '\x7f' || ((c as u32) >= 0x80 && (c as u32) < 0xa0) => {
                '\u{fffd}'
            }
            _ => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_plain_text() {
        assert_eq!(sanitize("hello world"), "hello world");
    }

    #[test]
    fn preserves_newlines_and_tabs() {
        assert_eq!(
            sanitize("line 1\nline 2\tindented"),
            "line 1\nline 2\tindented"
        );
    }

    #[test]
    fn strips_ansi_escape_csi() {
        // ESC [ 2 J — "clear screen" sequence
        let injected = "before\x1b[2Jafter";
        let clean = sanitize(injected);
        assert!(!clean.contains('\x1b'));
        assert!(clean.contains("before"));
        assert!(clean.contains("after"));
    }

    #[test]
    fn strips_bel_and_del() {
        assert!(!sanitize("\x07\x7f").contains('\x07'));
        assert!(!sanitize("\x07\x7f").contains('\x7f'));
    }

    #[test]
    fn strips_c1_controls() {
        // C1 CSI (\x9b) — the 8-bit version of ESC [
        let injected = "a\u{9b}2Jb";
        assert!(!sanitize(injected).contains('\u{9b}'));
    }
}
