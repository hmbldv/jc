//! Issue link types, listing, and creation.
//!
//! Issue links are directional: every link type has an `inward` and
//! `outward` phrase (e.g. "Blocks" → outward `blocks`, inward `is blocked
//! by`). Per Atlassian's REST v3 convention, `inwardIssue` is the source
//! of the outward verb ("blocks", "duplicates", …) and `outwardIssue` is
//! its target. So `link add <KEY> --to <OTHER> --type Blocks` meaning
//! "KEY blocks OTHER" maps to `inwardIssue = KEY, outwardIssue = OTHER`.

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
/// `from_key` is the source of the outward verb (the "blocker", "duplicator",
/// etc.); `to_key` is the recipient. Per Atlassian convention this maps to
/// `inwardIssue = from_key`, `outwardIssue = to_key`.
pub async fn add(client: &Client, link_type: &str, from_key: &str, to_key: &str) -> Result<()> {
    let req = CreateLinkRequest {
        link_type: LinkTypeRef { name: link_type },
        inward: IssueRef { key: from_key },
        outward: IssueRef { key: to_key },
    };
    client.post_no_content("rest/api/3/issueLink", &req).await
}

/// DELETE /rest/api/3/issueLink/{id}
pub async fn remove(client: &Client, link_id: &str) -> Result<()> {
    let path = format!("rest/api/3/issueLink/{link_id}");
    client.delete_no_content(&path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks in Atlassian's REST v3 convention for `POST /rest/api/3/issueLink`:
    /// `inwardIssue` is the source of the outward verb (the "blocker" for a
    /// `Blocks` link) and `outwardIssue` is its target (the "blocked" issue).
    ///
    /// The public `add(client, link_type, from_key, to_key)` is documented to
    /// map `from_key` → `inwardIssue` and `to_key` → `outwardIssue`. This test
    /// snapshots the exact JSON the typed request serializes to, so any future
    /// swap of the field assignments re-breaks the same 0.1.0 regression and
    /// fails here before reaching Jira.
    #[test]
    fn blocks_link_emits_from_as_inward_and_to_as_outward() {
        let req = CreateLinkRequest {
            link_type: LinkTypeRef { name: "Blocks" },
            inward: IssueRef { key: "FOO-1" }, // from_key / blocker
            outward: IssueRef { key: "FOO-2" }, // to_key / blocked
        };
        let body = serde_json::to_value(&req).unwrap();
        assert_eq!(
            body,
            serde_json::json!({
                "type": {"name": "Blocks"},
                "inwardIssue": {"key": "FOO-1"},
                "outwardIssue": {"key": "FOO-2"},
            })
        );
    }
}
