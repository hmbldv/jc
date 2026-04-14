//! JQL search via the new POST /rest/api/3/search/jql endpoint.
//!
//! The new Atlassian search API uses cursor pagination (`nextPageToken`)
//! instead of the old `startAt`-based scheme. This module hides the cursor
//! loop so callers see a single flat result set.

use jc_core::{Client, Result};
use serde::{Deserialize, Serialize};

use crate::issue::{IssueType, Priority, Status, User};

/// Lightweight hit for list/search results. Full issue data is available
/// through [`crate::issue::get`] using the hit's `key`.
#[derive(Debug, Deserialize)]
pub struct SearchHit {
    pub id: String,
    pub key: String,
    #[serde(default)]
    pub fields: SearchHitFields,
}

#[derive(Debug, Deserialize, Default)]
pub struct SearchHitFields {
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub status: Option<Status>,
    #[serde(default)]
    pub assignee: Option<User>,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub issuetype: Option<IssueType>,
    #[serde(default)]
    pub updated: Option<String>,
    #[serde(default)]
    pub labels: Vec<String>,
}

/// Default fields returned for search hits. Kept small so list responses
/// stay manageable; run `jc jira issue get <KEY>` for full issue data.
pub const DEFAULT_FIELDS: &[&str] = &[
    "summary",
    "status",
    "assignee",
    "priority",
    "issuetype",
    "updated",
    "labels",
];

#[derive(Debug, Serialize)]
struct SearchRequest<'a> {
    jql: &'a str,
    fields: &'a [&'a str],
    #[serde(rename = "maxResults")]
    max_results: usize,
    #[serde(rename = "nextPageToken", skip_serializing_if = "Option::is_none")]
    next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    issues: Vec<SearchHit>,
    #[serde(rename = "nextPageToken", default)]
    next_page_token: Option<String>,
    #[serde(rename = "isLast", default)]
    is_last: bool,
}

const PAGE_SIZE: usize = 100;

/// Execute a JQL query, auto-paginating through all result pages.
///
/// `limit = 0` means unlimited — collect every page. A positive `limit`
/// caps the result set and stops fetching as soon as it is reached.
pub async fn jql(
    client: &Client,
    query: &str,
    fields: &[&str],
    limit: usize,
) -> Result<Vec<SearchHit>> {
    let mut results: Vec<SearchHit> = Vec::new();
    let mut token: Option<String> = None;

    loop {
        let body = SearchRequest {
            jql: query,
            fields,
            max_results: PAGE_SIZE,
            next_page_token: token.clone(),
        };
        let resp: SearchResponse = client.post_json("rest/api/3/search/jql", &body).await?;
        let got = resp.issues.len();
        results.extend(resp.issues);

        if limit > 0 && results.len() >= limit {
            results.truncate(limit);
            break;
        }

        match resp.next_page_token {
            Some(t) if !resp.is_last && got > 0 => token = Some(t),
            _ => break,
        }
    }

    Ok(results)
}
