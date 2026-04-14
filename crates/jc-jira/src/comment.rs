use jc_core::{Client, Result};
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

#[derive(Debug, Serialize)]
struct AddCommentRequest<'a> {
    body: &'a Value,
}

/// POST /rest/api/3/issue/{key}/comment
///
/// Body must be an ADF document node. Use `jc_adf::to_adf(markdown)` to
/// produce it from a markdown string.
pub async fn add(client: &Client, issue_key: &str, body: &Value) -> Result<Comment> {
    let path = format!("rest/api/3/issue/{issue_key}/comment");
    let req = AddCommentRequest { body };
    client.post_json(&path, &req).await
}
