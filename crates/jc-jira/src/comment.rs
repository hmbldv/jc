use jc_core::{Client, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::issue::User;

#[derive(Debug, Deserialize)]
pub struct Comment {
    pub id: String,
    /// ADF body. Convert with `jc_adf::to_markdown`.
    #[serde(default)]
    pub body: Option<Value>,
    #[serde(default)]
    pub author: Option<User>,
    #[serde(default)]
    pub created: Option<String>,
    #[serde(default)]
    pub updated: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CommentList {
    #[serde(default)]
    comments: Vec<Comment>,
    #[serde(default)]
    total: u64,
}

#[derive(Debug, Serialize)]
struct CommentBodyRequest<'a> {
    body: &'a Value,
}

/// POST /rest/api/3/issue/{key}/comment
pub async fn add(client: &Client, issue_key: &str, body: &Value) -> Result<Comment> {
    let path = format!("rest/api/3/issue/{issue_key}/comment");
    client.post_json(&path, &CommentBodyRequest { body }).await
}

/// GET /rest/api/3/issue/{key}/comment/{id}
pub async fn get(client: &Client, issue_key: &str, id: &str) -> Result<Comment> {
    let path = format!("rest/api/3/issue/{issue_key}/comment/{id}");
    client.request_json(Method::GET, &path).await
}

/// GET /rest/api/3/issue/{key}/comment
///
/// Uses the old startAt/maxResults pagination scheme (the new cursor-based
/// API only covers the JQL search endpoint so far). `limit = 0` is unlimited.
pub async fn list(client: &Client, issue_key: &str, limit: usize) -> Result<Vec<Comment>> {
    const PAGE_SIZE: u64 = 100;
    let mut results: Vec<Comment> = Vec::new();
    let mut start_at: u64 = 0;

    loop {
        let path = format!(
            "rest/api/3/issue/{issue_key}/comment?startAt={start_at}&maxResults={PAGE_SIZE}"
        );
        let page: CommentList = client.request_json(Method::GET, &path).await?;
        let got = page.comments.len() as u64;
        let total = page.total;
        results.extend(page.comments);

        if limit > 0 && results.len() >= limit {
            results.truncate(limit);
            break;
        }
        if got == 0 || (results.len() as u64) >= total {
            break;
        }
        start_at += got;
    }

    Ok(results)
}

/// PUT /rest/api/3/issue/{key}/comment/{id}
pub async fn edit(
    client: &Client,
    issue_key: &str,
    id: &str,
    body: &Value,
) -> Result<Comment> {
    let path = format!("rest/api/3/issue/{issue_key}/comment/{id}");
    client.put_json(&path, &CommentBodyRequest { body }).await
}

/// DELETE /rest/api/3/issue/{key}/comment/{id}
pub async fn delete(client: &Client, issue_key: &str, id: &str) -> Result<()> {
    let path = format!("rest/api/3/issue/{issue_key}/comment/{id}");
    client.delete_no_content(&path).await
}
