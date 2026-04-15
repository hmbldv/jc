//! Confluence CQL search.
//!
//! CQL lives on the v1 `/wiki/rest/api/search` endpoint — it hasn't been
//! modernized into v2 yet, and Atlassian still recommends it as the primary
//! full-text search surface. Pagination is old-style `start`/`limit`.

use jc_core::{Client, Result};
use reqwest::Method;
use serde::Deserialize;
use url::form_urlencoded;

#[derive(Debug, Deserialize)]
pub struct SearchResult {
    #[serde(default)]
    pub content: Option<SearchContent>,
    #[serde(default)]
    pub excerpt: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default, rename = "lastModified")]
    pub last_modified: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct SearchContent {
    pub id: String,
    pub title: String,
    #[serde(rename = "type")]
    pub content_type: String,
    #[serde(rename = "spaceId", default)]
    pub space_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<SearchResult>,
    #[serde(default)]
    size: u64,
    #[serde(default, rename = "totalSize")]
    total_size: u64,
}

const PAGE_SIZE: u64 = 100;

/// Execute a CQL query, auto-paginating. `limit = 0` is unlimited.
pub async fn cql(client: &Client, query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let encoded: String = form_urlencoded::byte_serialize(query.as_bytes()).collect();
    let mut results: Vec<SearchResult> = Vec::new();
    let mut start: u64 = 0;

    loop {
        let path = format!("wiki/rest/api/search?cql={encoded}&start={start}&limit={PAGE_SIZE}");
        let resp: SearchResponse = client.request_json(Method::GET, &path).await?;
        let got = resp.size;
        let total = resp.total_size;
        results.extend(resp.results);

        if limit > 0 && results.len() >= limit {
            results.truncate(limit);
            break;
        }
        if got == 0 || (total > 0 && (results.len() as u64) >= total) {
            break;
        }
        start += got;
    }

    Ok(results)
}
