//! Issue link types, listing, and creation.
//!
//! Issue links are directional: every link type has an `inward` and
//! `outward` phrase (e.g. "Blocks" → outward `blocks`, inward `is blocked
//! by`). The CLI convention is `link add <KEY> --to <OTHER> --type Blocks`
//! meaning "KEY blocks OTHER", which maps to `outwardIssue = KEY,
//! inwardIssue = OTHER`.

use jc_core::{ApiError, Client, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct LinkType {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub inward: String,
    #[serde(default)]
    pub outward: String,
}

#[derive(Debug, Deserialize)]
struct LinkTypesResponse {
    #[serde(default, rename = "issueLinkTypes")]
    issue_link_types: Vec<LinkType>,
}

#[derive(Debug, Deserialize)]
pub struct IssueLink {
    pub id: String,
    #[serde(rename = "type")]
    pub link_type: LinkType,
    #[serde(rename = "inwardIssue", default)]
    pub inward_issue: Option<LinkedIssue>,
    #[serde(rename = "outwardIssue", default)]
    pub outward_issue: Option<LinkedIssue>,
}

#[derive(Debug, Deserialize)]
pub struct LinkedIssue {
    pub id: String,
    pub key: String,
    #[serde(default)]
    pub fields: Option<LinkedIssueFields>,
}

#[derive(Debug, Deserialize)]
pub struct LinkedIssueFields {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub status: Option<crate::issue::Status>,
}

#[derive(Debug, Serialize)]
struct CreateLinkRequest<'a> {
    #[serde(rename = "type")]
    link_type: LinkTypeRef<'a>,
    #[serde(rename = "inwardIssue")]
    inward: IssueRef<'a>,
    #[serde(rename = "outwardIssue")]
    outward: IssueRef<'a>,
}

#[derive(Debug, Serialize)]
struct LinkTypeRef<'a> {
    name: &'a str,
}

#[derive(Debug, Serialize)]
struct IssueRef<'a> {
    key: &'a str,
}

/// GET /rest/api/3/issueLinkType — all configured link types.
pub async fn list_types(client: &Client) -> Result<Vec<LinkType>> {
    let resp: LinkTypesResponse = client
        .request_json(Method::GET, "rest/api/3/issueLinkType")
        .await?;
    Ok(resp.issue_link_types)
}

/// Fetch `issuelinks` for a single issue and parse into typed form.
pub async fn list_on_issue(client: &Client, issue_key: &str) -> Result<Vec<IssueLink>> {
    let path = format!("rest/api/3/issue/{issue_key}?fields=issuelinks");
    let v: Value = client.request_json(Method::GET, &path).await?;
    let links = v
        .get("fields")
        .and_then(|f| f.get("issuelinks"))
        .cloned()
        .unwrap_or_else(|| Value::Array(vec![]));
    serde_json::from_value(links).map_err(ApiError::decode)
}

/// POST /rest/api/3/issueLink
///
/// `outward_key` is the "from" side (the one that does the action implied
/// by the link type — "blocks", "duplicates", etc.); `inward_key` is the
/// "to" side.
pub async fn add(
    client: &Client,
    link_type: &str,
    outward_key: &str,
    inward_key: &str,
) -> Result<()> {
    let req = CreateLinkRequest {
        link_type: LinkTypeRef { name: link_type },
        inward: IssueRef { key: inward_key },
        outward: IssueRef { key: outward_key },
    };
    client.post_no_content("rest/api/3/issueLink", &req).await
}

/// DELETE /rest/api/3/issueLink/{id}
pub async fn remove(client: &Client, link_id: &str) -> Result<()> {
    let path = format!("rest/api/3/issueLink/{link_id}");
    client.delete_no_content(&path).await
}
