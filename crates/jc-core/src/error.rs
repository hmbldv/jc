use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub type Result<T> = std::result::Result<T, ApiError>;

/// Structured error type. Serializable so the CLI layer can emit it verbatim
/// on stderr as JSON.
#[derive(Debug, Error, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ApiError {
    #[error("HTTP {status}: {}", messages.join("; "))]
    Api {
        status: u16,
        code: String,
        messages: Vec<String>,
        #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
        field_errors: std::collections::BTreeMap<String, String>,
    },
    #[error("transport: {message}")]
    Transport { message: String },
    #[error("decode: {message}")]
    Decode { message: String },
    #[error("url: {message}")]
    Url { message: String },
    #[error("config: {message}")]
    Config { message: String },
}

#[derive(Debug, Deserialize, Default)]
struct AtlassianErrorBody {
    #[serde(default, rename = "errorMessages")]
    error_messages: Vec<String>,
    #[serde(default)]
    errors: std::collections::BTreeMap<String, String>,
}

impl ApiError {
    pub fn transport(e: reqwest::Error) -> Self {
        Self::Transport {
            message: e.to_string(),
        }
    }

    pub fn decode(e: serde_json::Error) -> Self {
        Self::Decode {
            message: e.to_string(),
        }
    }

    pub fn url(e: url::ParseError) -> Self {
        Self::Url {
            message: e.to_string(),
        }
    }

    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config {
            message: msg.into(),
        }
    }

    pub fn from_response(status: StatusCode, body: &[u8]) -> Self {
        let parsed: AtlassianErrorBody = serde_json::from_slice(body).unwrap_or_default();
        let messages = if parsed.error_messages.is_empty() {
            vec![String::from_utf8_lossy(body).into_owned()]
        } else {
            parsed.error_messages
        };
        Self::Api {
            status: status.as_u16(),
            code: status.canonical_reason().unwrap_or("ERROR").to_string(),
            messages,
            field_errors: parsed.errors,
        }
    }
}
