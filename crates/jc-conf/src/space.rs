//! Confluence v2 spaces.
//!
//! Users refer to spaces by human key (e.g. `ENG`), but every v2 write
//! endpoint wants the numeric `spaceId`. `find_by_key` resolves the key
//! to a full space record in one round trip.

use jc_core::{ApiError, Client, Result};
use reqwest::Method;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Space {
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(default, rename = "type")]
    pub space_type: Option<String>,
    #[serde(default, rename = "homepageId")]
    pub homepage_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SpaceList {
    #[serde(default)]
    results: Vec<Space>,
}

/// GET /wiki/api/v2/spaces (optionally filtered by keys)
pub async fn list(client: &Client, keys: &[&str]) -> Result<Vec<Space>> {
    let mut path = String::from("wiki/api/v2/spaces?limit=250");
    if !keys.is_empty() {
        path.push_str("&keys=");
        path.push_str(&keys.join(","));
    }
    let resp: SpaceList = client.request_json(Method::GET, &path).await?;
    Ok(resp.results)
}

/// Look up a single space by key. Returns `None` if no space matches.
pub async fn find_by_key(client: &Client, key: &str) -> Result<Option<Space>> {
    let spaces = list(client, &[key]).await?;
    Ok(spaces.into_iter().next())
}

/// Resolve a space key to its numeric ID, erroring if no match is found.
pub async fn resolve_id(client: &Client, key: &str) -> Result<String> {
    find_by_key(client, key)
        .await?
        .map(|s| s.id)
        .ok_or_else(|| ApiError::config(format!("space '{key}' not found")))
}

/// GET /wiki/api/v2/spaces/{id}
pub async fn get(client: &Client, id: &str) -> Result<Space> {
    let path = format!("wiki/api/v2/spaces/{id}");
    client.request_json(Method::GET, &path).await
}
