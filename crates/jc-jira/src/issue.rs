use jc_core::{Client, Result};
use reqwest::Method;
use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct Issue {
    pub id: String,
    pub key: String,
    pub fields: IssueFields,
}

#[derive(Debug, Deserialize)]
pub struct IssueFields {
    pub summary: String,
    /// ADF document. Convert with `jc_adf::to_markdown`.
    #[serde(default)]
    pub description: Option<Value>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub assignee: Option<User>,
    #[serde(default)]
    pub reporter: Option<User>,
    #[serde(default)]
    pub issuetype: Option<IssueType>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub labels: Vec<String>,
    #[serde(default)]
    pub comment: Option<CommentContainer>,
    #[serde(default)]
    pub attachment: Vec<Attachment>,
    #[serde(default, rename = "issuelinks")]
    pub issue_links: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct Status {
    pub name: String,
    #[serde(rename = "statusCategory", default)]
    pub category: Option<StatusCategory>,
}

#[derive(Debug, Deserialize)]
pub struct StatusCategory {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct User {
    #[serde(rename = "accountId")]
    pub account_id: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "emailAddress", default)]
    pub email_address: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct IssueType {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct Priority {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct CommentContainer {
    pub total: u64,
}

#[derive(Debug, Deserialize)]
pub struct Attachment {
    pub id: String,
    pub filename: String,
    #[serde(rename = "mimeType", default)]
    pub mime_type: Option<String>,
    pub size: u64,
}

/// GET /rest/api/3/issue/{key}
///
/// Requests `fields=*all` so the response includes description, attachments,
/// comment counts, and all standard fields in one round trip.
pub async fn get(client: &Client, key: &str) -> Result<Issue> {
    let path = format!("rest/api/3/issue/{key}?fields=*all");
    client.request_json(Method::GET, &path).await
}

#[derive(Debug, Deserialize)]
pub struct CreateResult {
    pub id: String,
    pub key: String,
    #[serde(default, rename = "self")]
    pub self_url: Option<String>,
}

/// POST /rest/api/3/issue
///
/// `fields` is the `fields` object that will be wrapped inside the request
/// envelope — callers should populate custom fields with their resolved
/// `customfield_XXXXX` IDs.
pub async fn create(client: &Client, fields: &Value) -> Result<CreateResult> {
    let body = serde_json::json!({ "fields": fields });
    client.post_json("rest/api/3/issue", &body).await
}

/// PUT /rest/api/3/issue/{key}
///
/// Returns 204 No Content on success. Same `fields` shape as `create`.
pub async fn edit(client: &Client, key: &str, fields: &Value) -> Result<()> {
    let path = format!("rest/api/3/issue/{key}");
    let body = serde_json::json!({ "fields": fields });
    client.put_no_content(&path, &body).await
}

/// PUT /rest/api/3/issue/{key}/assignee
///
/// Pass `None` to unassign. Jira accepts `{"accountId": null}`.
pub async fn assign(client: &Client, key: &str, account_id: Option<&str>) -> Result<()> {
    let path = format!("rest/api/3/issue/{key}/assignee");
    let body = serde_json::json!({ "accountId": account_id });
    client.put_no_content(&path, &body).await
}

/// POST /rest/api/3/issue/{key}/watchers
///
/// Body is a bare JSON string containing the accountId (this is genuinely
/// the endpoint's shape — not an object).
pub async fn add_watcher(client: &Client, key: &str, account_id: &str) -> Result<()> {
    let path = format!("rest/api/3/issue/{key}/watchers");
    client.post_no_content(&path, &account_id).await
}

/// DELETE /rest/api/3/issue/{key}/watchers?accountId=...
pub async fn remove_watcher(client: &Client, key: &str, account_id: &str) -> Result<()> {
    let encoded: String =
        url::form_urlencoded::byte_serialize(account_id.as_bytes()).collect();
    let path = format!("rest/api/3/issue/{key}/watchers?accountId={encoded}");
    client.delete_no_content(&path).await
}
