//! Dry-run / confirm harness.
//!
//! Mutating command handlers build a [`Preview`] describing the HTTP request
//! they are about to send and route it through [`PreviewMode`]:
//!
//! - [`PreviewMode::DryRun`] — emit preview as JSON on stdout, exit 0, no send
//! - [`PreviewMode::Confirm`] — print preview to stderr, block on stdin y/N
//! - [`PreviewMode::Send`] — send immediately, no preview
//!
//! The preview format is identical across all modes so Claude Code can always
//! do `--dry-run` first, show the user, and re-run without the flag once the
//! user approves.

use std::io::{BufRead, IsTerminal, Write};

use serde::Serialize;
use serde_json::{Value, json};

use crate::output::{CliError, Envelope};
use crate::sanitize::sanitize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewMode {
    Send,
    DryRun,
    Confirm,
}

impl PreviewMode {
    pub fn from_flags(dry_run: bool, confirm: bool) -> Self {
        if dry_run {
            Self::DryRun
        } else if confirm {
            Self::Confirm
        } else {
            Self::Send
        }
    }
}

/// Structured description of an outgoing HTTP request.
///
/// Headers are the rendered request headers minus Authorization, which is
/// replaced with `"Basic ***"` so previews never leak the API token.
#[derive(Debug, Serialize)]
pub struct Preview {
    pub method: String,
    pub url: String,
    pub headers: serde_json::Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Unified diff against current remote state, for edit operations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<String>,
}

impl Preview {
    pub fn new(method: &str, url: String) -> Self {
        let mut headers = serde_json::Map::new();
        headers.insert("Accept".into(), json!("application/json"));
        headers.insert("Content-Type".into(), json!("application/json"));
        headers.insert("Authorization".into(), json!("Basic ***"));
        Self {
            method: method.to_string(),
            url,
            headers,
            body: None,
            summary: None,
            diff: None,
        }
    }

    pub fn with_body(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_diff(mut self, diff: String) -> Self {
        self.diff = Some(diff);
        self
    }

    /// Dry-run output: the preview itself as the envelope data, with a
    /// `mode: "dry_run"` meta entry so Claude Code can tell it wasn't a
    /// real mutation response.
    pub fn emit_dry_run(&self) {
        let data = json!({ "preview": self, "will_send": false });
        let mut env = Envelope::new(data);
        let mut meta = serde_json::Map::new();
        meta.insert("mode".into(), json!("dry_run"));
        env.meta = Some(Value::Object(meta));
        env.emit();
    }

    /// Render the preview to stderr without prompting. Used by composite
    /// commands that stitch multiple previews together before asking for
    /// a single confirmation.
    ///
    /// All free-form string fields (`summary`, `url`, `diff`) are passed
    /// through `sanitize()` before writing to stderr. Server-controlled
    /// content can contain ANSI escape sequences; unsanitized writes to
    /// a TTY let a hostile Jira comment rewrite the confirmation prompt.
    pub fn render_to_stderr(&self) -> Result<(), CliError> {
        if let Some(summary) = &self.summary {
            eprintln!("# {}", sanitize(summary));
        }
        eprintln!("{} {}", sanitize(&self.method), sanitize(&self.url));
        if let Some(diff) = &self.diff {
            eprintln!("\n--- diff ---\n{}", sanitize(diff));
        }
        if let Some(body) = &self.body {
            // serde_json's string encoder already escapes control chars
            // inside JSON strings, so this path is safe without extra
            // sanitization — the output is structured JSON, not a free
            // concatenation of server content.
            let rendered = serde_json::to_string_pretty(body)
                .map_err(|e| CliError::validation(format!("serialize preview: {e}")))?;
            eprintln!("\n--- body ---\n{rendered}");
        }
        Ok(())
    }

    /// Confirm mode: render the preview to stderr and block on stdin.
    /// Returns `true` if the user typed `y`/`yes`, `false` otherwise.
    ///
    /// Errors immediately if stdin is not a terminal — a piped or closed
    /// stdin would silently decline every prompt, which is a footgun
    /// when `--confirm` is used in a wrapper script. `--dry-run` is the
    /// correct non-interactive preview mode.
    pub fn confirm_interactive(&self) -> Result<bool, CliError> {
        if !std::io::stdin().is_terminal() {
            return Err(CliError::validation(
                "--confirm requires an interactive terminal (stdin is not a tty); \
                 use --dry-run for non-interactive previews",
            ));
        }
        eprintln!("--- preview ---");
        self.render_to_stderr()?;
        prompt_yes_no("Send? [y/N]: ")
    }
}

/// Emit a composite dry-run envelope containing multiple planned requests.
pub fn emit_composite_dry_run(previews: &[Preview]) {
    let previews_json: Vec<Value> = previews
        .iter()
        .map(|p| serde_json::to_value(p).unwrap_or(Value::Null))
        .collect();
    let data = json!({
        "previews": previews_json,
        "will_send": false,
    });
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("mode".into(), json!("dry_run"));
    meta.insert("step_count".into(), json!(previews.len()));
    env.meta = Some(Value::Object(meta));
    env.emit();
}

/// Prompt on stderr and read a single y/N answer from stdin.
pub fn prompt_yes_no(prompt: &str) -> Result<bool, CliError> {
    eprint!("\n{prompt}");
    std::io::stderr().flush().ok();
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| CliError::io(format!("read stdin: {e}")))?;
    let answer = line.trim();
    Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
}
