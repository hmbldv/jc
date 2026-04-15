use std::process::ExitCode;

use jc_core::ApiError;
use serde::Serialize;
use serde_json::{Value, json};

/// Envelope for successful command output.
#[derive(Debug, Serialize)]
pub struct Envelope<T: Serialize> {
    pub data: T,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl<T: Serialize> Envelope<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            warnings: vec![],
            meta: None,
        }
    }

    pub fn emit(self) {
        match serde_json::to_string_pretty(&self) {
            Ok(s) => println!("{s}"),
            Err(e) => eprintln!(
                "{{\"error\":{{\"kind\":\"encode\",\"message\":{:?}}}}}",
                e.to_string()
            ),
        }
    }
}

/// Top-level error type for the CLI.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("io: {message}")]
    Io { message: String },
    #[error("validation: {message}")]
    Validation { message: String },
}

impl CliError {
    pub fn io(message: impl Into<String>) -> Self {
        Self::Io {
            message: message.into(),
        }
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation {
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        match self {
            CliError::Api(ApiError::Config { .. }) => ExitCode::from(3),
            CliError::Api(_) => ExitCode::from(2),
            CliError::Io { .. } => ExitCode::from(1),
            CliError::Validation { .. } => ExitCode::from(4),
        }
    }
}

pub fn emit_error(err: &CliError) {
    let body = match err {
        CliError::Api(e) => json!({ "error": e }),
        CliError::Io { message } => json!({
            "error": { "kind": "io", "message": message }
        }),
        CliError::Validation { message } => json!({
            "error": { "kind": "validation", "message": message }
        }),
    };
    let rendered = serde_json::to_string_pretty(&body)
        .unwrap_or_else(|_| r#"{"error":{"kind":"encode"}}"#.to_string());
    eprintln!("{rendered}");
}
