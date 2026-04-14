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

use std::io::{BufRead, Write};

use serde::Serialize;
use serde_json::{Value, json};

use crate::output::{CliError, Envelope};

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

    /// Confirm mode: render the preview to stderr and block on stdin.
    /// Returns `true` if the user typed `y`/`yes`, `false` otherwise.
    pub fn confirm_interactive(&self) -> Result<bool, CliError> {
        let rendered = serde_json::to_string_pretty(self)
            .map_err(|e| CliError::validation(format!("serialize preview: {e}")))?;
        eprintln!("--- preview ---");
        eprintln!("{rendered}");
        eprint!("Send? [y/N]: ");
        std::io::stderr().flush().ok();

        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| CliError::io(format!("read stdin: {e}")))?;
        let answer = line.trim();
        Ok(answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes"))
    }
}
