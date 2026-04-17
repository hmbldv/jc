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

/// Build the JSON body for `POST /rest/api/3/issueLink`.
///
/// Canonical builder used by both `add()` and the CLI dry-run preview, so the
/// two paths cannot drift in direction semantics (the 0.1.0 regression was
/// two independent hand-written bodies disagreeing with Atlassian's convention).
///
/// `from_key` is the source of the outward verb (the "blocker", "duplicator",
/// etc.); `to_key` is the recipient. Per Atlassian convention this maps to
/// `inwardIssue = from_key`, `outwardIssue = to_key`.
pub fn build_add_request_body(link_type: &str, from_key: &str, to_key: &str) -> Value {
    let req = CreateLinkRequest {
        link_type: LinkTypeRef { name: link_type },
        inward: IssueRef { key: from_key },
        outward: IssueRef { key: to_key },
    };
    serde_json::to_value(&req).expect("CreateLinkRequest serializes to Value infallibly")
}

/// POST /rest/api/3/issueLink
///
/// See [`build_add_request_body`] for parameter direction semantics.
pub async fn add(client: &Client, link_type: &str, from_key: &str, to_key: &str) -> Result<()> {
    let body = build_add_request_body(link_type, from_key, to_key);
    client.post_no_content("rest/api/3/issueLink", &body).await
}

/// DELETE /rest/api/3/issueLink/{id}
pub async fn remove(client: &Client, link_id: &str) -> Result<()> {
    let path = format!("rest/api/3/issueLink/{link_id}");
    client.delete_no_content(&path).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The direction invariant: `from_key` → `inwardIssue`, `to_key` → `outwardIssue`.
    ///
    /// Tests go through [`build_add_request_body`] — the same helper `add()`
    /// uses — so a future swap of the parameter-to-field mapping (the 0.1.0
    /// regression) fails here before reaching Jira. Tests that snapshot a
    /// hand-constructed `CreateLinkRequest` wouldn't catch such a swap
    /// because they bypass the parameter-ordering layer.
    #[test]
    fn from_key_maps_to_inward_issue_and_to_key_maps_to_outward_issue() {
        let body = build_add_request_body("Blocks", "FOO-1", "FOO-2");
        assert_eq!(body["inwardIssue"]["key"], "FOO-1");
        assert_eq!(body["outwardIssue"]["key"], "FOO-2");
    }

    /// Full wire-format snapshot. Guards against accidental field renames
    /// (e.g. if someone drops `#[serde(rename = "inwardIssue")]`) on top of
    /// the direction invariant above.
    #[test]
    fn blocks_link_snapshot() {
        let body = build_add_request_body("Blocks", "FOO-1", "FOO-2");
        assert_eq!(
            body,
            serde_json::json!({
                "type": {"name": "Blocks"},
                "inwardIssue": {"key": "FOO-1"},
                "outwardIssue": {"key": "FOO-2"},
            })
        );
    }

    /// The payload mapping is type-agnostic, but the CHANGELOG explicitly
    /// calls out `Duplicate` and `Clones` as affected by the 0.1.0 bug, so
    /// make their correctness explicit too. `Relates` is symmetric in Jira's
    /// default config, so user-visible direction doesn't matter — but the
    /// wire format must still follow the same rule.
    #[test]
    fn direction_invariant_holds_across_link_types() {
        for link_type in ["Blocks", "Duplicate", "Clones", "Relates"] {
            let body = build_add_request_body(link_type, "A-1", "B-2");
            assert_eq!(
                body["inwardIssue"]["key"], "A-1",
                "{link_type}: from_key must map to inwardIssue"
            );
            assert_eq!(
                body["outwardIssue"]["key"], "B-2",
                "{link_type}: to_key must map to outwardIssue"
            );
            assert_eq!(body["type"]["name"], link_type);
        }
    }
}
