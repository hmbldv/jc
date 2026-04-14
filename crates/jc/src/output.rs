use std::process::ExitCode;

use jc_core::ApiError;
use serde::Serialize;

/// Envelope for successful command output.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub data: T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

impl<T: Serialize> Envelope<T> {
    pub fn new(data: T) -> Self {
        Self { data, warnings: vec![], meta: None }
    }

    pub fn emit(self) {
        // Pretty-print for human readability; Claude Code handles either.
        // JSON parsers don't care about whitespace.
        match serde_json::to_string_pretty(&self) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!("{{\"error\":{{\"kind\":\"encode\",\"message\":{:?}}}}}", e.to_string()),
        }
    }
}

/// Top-level error type for the CLI. Wraps jc-core errors and adds CLI-only
/// variants (e.g. dry-run rejection, usage errors).
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Api(#[from] ApiError),
}

impl CliError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            CliError::Api(ApiError::Config { .. }) => ExitCode::from(3),
            CliError::Api(_) => ExitCode::from(2),
        }
    }
}

pub fn emit_error(err: &CliError) {
    let body = serde_json::json!({ "error": err.api() });
    let rendered = serde_json::to_string_pretty(&body)
        .unwrap_or_else(|_| r#"{"error":{"kind":"encode"}}"#.to_string());
    eprintln!("{rendered}");
}

impl CliError {
    fn api(&self) -> &ApiError {
        match self {
            CliError::Api(e) => e,
        }
    }
}
