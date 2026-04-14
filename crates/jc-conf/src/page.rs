//! Confluence Cloud REST v2 pages.
//!
//! Reads request `body-format=atlas_doc_format` so page bodies come back
//! as ADF, which we can then turn into markdown with `jc_adf::to_markdown`.
//!
//! Writes go through the v2 body envelope
//! `{"representation": "atlas_doc_format", "value": "<ADF JSON string>"}`.
//! The `value` field is a JSON string, not a JSON object — the ADF tree has
//! to be serialized once more before being handed to the endpoint.

use jc_core::{Client, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Page {
    pub id: String,
    pub title: String,
    #[serde(rename = "spaceId")]
    pub space_id: String,
    #[serde(rename = "parentId", default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub version: Option<Version>,
    #[serde(default)]
    pub body: Option<PageBody>,
    #[serde(rename = "authorId", default)]
    pub author_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Version {
    pub number: u64,
    #[serde(rename = "createdAt", default)]
    pub created_at: Option<String>,
    #[serde(rename = "authorId", default)]
    pub author_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PageBody {
    #[serde(default, rename = "atlas_doc_format")]
    pub atlas_doc_format: Option<BodyValue>,
}

#[derive(Debug, Deserialize)]
pub struct BodyValue {
    pub representation: String,
    /// ADF document serialized as a JSON string.
    pub value: String,
}

impl PageBody {
    /// Parse the embedded ADF value string into a concrete ADF document.
    pub fn as_adf(&self) -> Option<Value> {
        self.atlas_doc_format
            .as_ref()
            .and_then(|b| serde_json::from_str(&b.value).ok())
    }
}

#[derive(Debug, Deserialize)]
pub struct PageSummary {
    pub id: String,
    pub title: String,
    #[serde(rename = "spaceId", default)]
    pub space_id: Option<String>,
    #[serde(rename = "parentId", default)]
    pub parent_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, rename = "createdAt")]
    pub created_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PageList {
    #[serde(default)]
    results: Vec<PageSummary>,
    #[serde(rename = "_links", default)]
    links: Option<Links>,
}

#[derive(Debug, Deserialize)]
struct Links {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreatePageRequest<'a> {
    #[serde(rename = "spaceId")]
    pub space_id: &'a str,
    pub status: &'static str,
    pub title: &'a str,
    #[serde(rename = "parentId", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<&'a str>,
    pub body: BodyRequest,
}

#[derive(Debug, Serialize)]
pub struct UpdatePageRequest<'a> {
    pub id: &'a str,
    pub status: &'static str,
    pub title: &'a str,
    pub version: VersionRequest,
    pub body: BodyRequest,
}

#[derive(Debug, Serialize)]
pub struct BodyRequest {
    pub representation: &'static str,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct VersionRequest {
    pub number: u64,
}

impl BodyRequest {
    /// Build the v2 body envelope from an ADF document, serializing the
    /// tree to a JSON string as the endpoint requires.
    pub fn from_adf(adf: &Value) -> Self {
        Self {
            representation: "atlas_doc_format",
            value: serde_json::to_string(adf).unwrap_or_else(|_| "{}".to_string()),
        }
    }
}

/// GET /wiki/api/v2/pages/{id}?body-format=atlas_doc_format
pub async fn get(client: &Client, id: &str) -> Result<Page> {
    let path = format!("wiki/api/v2/pages/{id}?body-format=atlas_doc_format");
    client.request_json(Method::GET, &path).await
}

/// List pages in a space, or direct children of a specific parent page.
///
/// Uses v2 cursor pagination via `_links.next`. `limit = 0` is unlimited.
pub async fn list(
    client: &Client,
    space_id: &str,
    parent_id: Option<&str>,
    limit: usize,
) -> Result<Vec<PageSummary>> {
    let mut results: Vec<PageSummary> = Vec::new();
    let mut path = if let Some(parent) = parent_id {
        format!("wiki/api/v2/pages/{parent}/children?limit=250")
    } else {
        format!("wiki/api/v2/spaces/{space_id}/pages?limit=250")
    };

    loop {
        let page: PageList = client.request_json(Method::GET, &path).await?;
        let got = page.results.len();
        results.extend(page.results);

        if limit > 0 && results.len() >= limit {
            results.truncate(limit);
            break;
        }

        match page.links.and_then(|l| l.next) {
            Some(next) if got > 0 => {
                path = next
                    .trim_start_matches('/')
                    .trim_start_matches("wiki/")
                    .to_string();
                // url::Url::join strips the base path if we re-add "wiki/";
                // the Client's base is the site root so re-prefix it.
                path = format!("wiki/{path}");
            }
            _ => break,
        }
    }

    Ok(results)
}

/// POST /wiki/api/v2/pages
pub async fn create(client: &Client, req: &CreatePageRequest<'_>) -> Result<Page> {
    client.post_json("wiki/api/v2/pages", req).await
}

/// PUT /wiki/api/v2/pages/{id}
pub async fn update(client: &Client, id: &str, req: &UpdatePageRequest<'_>) -> Result<Page> {
    let path = format!("wiki/api/v2/pages/{id}");
    client.put_json(&path, req).await
}

/// DELETE /wiki/api/v2/pages/{id}
pub async fn delete(client: &Client, id: &str) -> Result<()> {
    let path = format!("wiki/api/v2/pages/{id}");
    client.delete_no_content(&path).await
}
