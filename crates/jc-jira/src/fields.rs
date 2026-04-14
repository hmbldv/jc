//! Jira custom field name↔ID resolution.
//!
//! Jira exposes custom fields as `customfield_10042`-style IDs which mean
//! nothing to a human. This module fetches the full field catalog once,
//! caches it to `~/.cache/jc/fields.json`, and provides a small lookup
//! helper so command handlers accept human names everywhere.
//!
//! The cache is pure — never authoritative, rebuildable at any time.
//! `jc jira fields sync` refreshes it on demand.

use jc_core::{Client, Result, cache};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const CACHE_FILE: &str = "fields.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Field {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub custom: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub struct FieldsCache {
    pub fields: Vec<Field>,
}

impl FieldsCache {
    /// Resolve a human name or raw ID to a field ID. Case-insensitive on
    /// names; always a passthrough on IDs that already exist.
    pub fn resolve_id<'a>(&'a self, name_or_id: &'a str) -> Option<&'a str> {
        if self.fields.iter().any(|f| f.id == name_or_id) {
            return Some(name_or_id);
        }
        let lower = name_or_id.to_ascii_lowercase();
        self.fields
            .iter()
            .find(|f| f.name.to_ascii_lowercase() == lower)
            .map(|f| f.id.as_str())
    }

    /// Load the cache from disk, returning an empty cache if missing.
    pub fn load() -> Self {
        cache::read_json(CACHE_FILE).unwrap_or_default()
    }

    /// Persist the cache to disk, returning the file path written.
    pub fn save(&self) -> std::io::Result<std::path::PathBuf> {
        cache::write_json(CACHE_FILE, self)
    }
}

/// GET /rest/api/3/field — fetches the full field catalog. Used by
/// `jc jira fields sync` to refresh the on-disk cache.
pub async fn list_all(client: &Client) -> Result<Vec<Field>> {
    client.request_json(Method::GET, "rest/api/3/field").await
}
