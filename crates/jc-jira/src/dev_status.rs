//! Jira "development status" panel — the branches, commits, pull requests,
//! and builds linked to an issue through dev-tool integrations
//! (GitHub / Bitbucket / GitLab / …).
//!
//! Backed by the `/rest/dev-status/1.0/` API. Atlassian does not formally
//! document or support this endpoint, but the Jira Cloud UI itself drives
//! its development panel from it, so it is stable in practice.

use jc_core::{Client, Result};
use reqwest::Method;
use serde_json::Value;

/// GET /rest/dev-status/1.0/issue/summary?issueId={id}
///
/// Returns the raw summary body. `issue_id` is the issue's numeric id (see
/// [`crate::issue::get_id`]), not the human-readable key.
pub async fn summary(client: &Client, issue_id: &str) -> Result<Value> {
    let path = format!("rest/dev-status/1.0/issue/summary?issueId={issue_id}");
    client.request_json(Method::GET, &path).await
}

/// GET /rest/dev-status/1.0/issue/detail?issueId={id}&dataType={t}&applicationType={app}
///
/// Returns the raw detail body for one provider and data type. Both
/// `data_type` and `application_type` are required by the endpoint.
pub async fn detail(
    client: &Client,
    issue_id: &str,
    data_type: &str,
    application_type: &str,
) -> Result<Value> {
    let path = format!(
        "rest/dev-status/1.0/issue/detail?issueId={issue_id}&dataType={data_type}&applicationType={application_type}"
    );
    client.request_json(Method::GET, &path).await
}

/// Provider `applicationType` slugs that report data for `data_type` in a
/// summary response, ready to feed the detail endpoint verbatim. `data_type` is
/// the *detail* data type (`pullrequest`, `branch`, `build`).
///
/// The `byInstanceType` *keys* are themselves the `applicationType` values the
/// detail endpoint expects (e.g. `bitbucket`, `stash`, `oAuth-mR7e…`,
/// `cloud-providers`) and are case-sensitive, so they are returned unchanged —
/// not the human-readable `name`. Returns an empty vec when the data type is
/// absent or no instance has data.
pub fn providers_with_data(summary: &Value, data_type: &str) -> Vec<String> {
    let Some(by_instance) = summary
        .get("summary")
        .and_then(|s| s.get(data_type))
        .and_then(|t| t.get("byInstanceType"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };

    by_instance
        .iter()
        .filter(|(_, v)| v.get("count").and_then(Value::as_u64).unwrap_or(0) > 0)
        .map(|(slug, _)| slug.clone())
        .collect()
}

/// What a summary response tells us about the issue's development data, used
/// to word the user-facing warning (Jira returns an empty body for both "no
/// data" and "no integration", so we lean on the `errors`/`configErrors`
/// signal to tell them apart).
#[derive(Debug, PartialEq, Eq)]
pub enum IntegrationState {
    /// At least one data type reports a non-zero count.
    HasData,
    /// Integration present, but nothing is linked to this issue.
    NoData,
    /// No dev-tool integration configured, or the caller lacks the
    /// "View development tools" permission — the data cannot be trusted.
    NoIntegration,
}

/// Classify a raw summary response. Treats any non-empty `errors`/`configErrors`
/// as [`IntegrationState::NoIntegration`]; otherwise reports whether any data
/// type has a non-zero overall count.
pub fn integration_state(summary: &Value) -> IntegrationState {
    let has_errors = |key: &str| {
        summary
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
    };
    if has_errors("errors") || has_errors("configErrors") {
        return IntegrationState::NoIntegration;
    }

    let has_counts = summary
        .get("summary")
        .and_then(Value::as_object)
        .is_some_and(|types| {
            types.values().any(|t| {
                t.get("overall")
                    .and_then(|o| o.get("count"))
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    > 0
            })
        });

    if has_counts {
        IntegrationState::HasData
    } else {
        IntegrationState::NoData
    }
}

/// Curated projection of a summary response: one entry per data type that is
/// present, each carrying its overall `count` and (where the type has one) its
/// `state`. Absent data types are omitted.
pub fn project_summary(summary: &Value) -> Value {
    let Some(types) = summary.get("summary").and_then(Value::as_object) else {
        return serde_json::json!({});
    };

    let mut out = serde_json::Map::new();
    for (name, ty) in types {
        let overall = ty.get("overall");
        let count = overall
            .and_then(|o| o.get("count"))
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let mut entry = serde_json::Map::new();
        entry.insert("count".to_string(), count.into());
        if let Some(state) = overall
            .and_then(|o| o.get("state"))
            .filter(|s| !s.is_null())
        {
            entry.insert("state".to_string(), state.clone());
        }
        out.insert(name.clone(), Value::Object(entry));
    }
    Value::Object(out)
}

/// Curated pull requests flattened across every instance in a detail response.
/// Each entry: `id, title, url, status, author, source_branch, dest_branch,
/// last_update`.
pub fn project_pull_requests(detail: &Value) -> Vec<Value> {
    flatten_detail(detail, "pullRequests")
        .map(|pr| {
            serde_json::json!({
                "id": pr.get("id"),
                "title": pr.get("name"),
                "url": pr.get("url"),
                "status": pr.get("status"),
                "author": pr.get("author").and_then(|a| a.get("name")),
                "source_branch": pr.get("source").and_then(|s| s.get("branch")),
                "dest_branch": pr.get("destination").and_then(|d| d.get("branch")),
                "last_update": pr.get("lastUpdate"),
            })
        })
        .collect()
}

/// Curated branches flattened across every instance in a detail response. Each
/// entry: `name, url, repo, last_commit{hash, message, date}`. The owning
/// repository name travels on each branch under `repository.name`.
pub fn project_branches(detail: &Value) -> Vec<Value> {
    flatten_detail(detail, "branches")
        .map(|branch| {
            let commit = branch.get("lastCommit");
            serde_json::json!({
                "name": branch.get("name"),
                "url": branch.get("url"),
                "repo": branch.get("repository").and_then(|r| r.get("name")),
                "last_commit": {
                    "hash": commit.and_then(|c| c.get("id")),
                    "message": commit.and_then(|c| c.get("message")),
                    "date": commit.and_then(|c| c.get("authorTimestamp")),
                },
            })
        })
        .collect()
}

/// Curated builds flattened across every instance in a detail response. Each
/// entry: `name, url, state, last_update`.
///
/// Two wire shapes are merged: the legacy dev-status layout puts builds at
/// `detail[].builds[]` (with a `name`), while modern Forge/cloud DevOps
/// providers nest them at `detail[].jswddBuildsData[].builds[]` (with a
/// `displayName`). Both are collected; the name falls back across the two
/// field names.
pub fn project_builds(detail: &Value) -> Vec<Value> {
    let legacy = flatten_detail(detail, "builds");
    let modern = flatten_detail(detail, "jswddBuildsData")
        .filter_map(|d| d.get("builds").and_then(Value::as_array))
        .flatten();

    legacy
        .chain(modern)
        .map(|b| {
            serde_json::json!({
                "name": b.get("displayName").or_else(|| b.get("name")),
                "url": b.get("url"),
                "state": b.get("state"),
                "last_update": b.get("lastUpdated"),
            })
        })
        .collect()
}

/// Iterate every record under `detail[].{key}` across all instances.
fn flatten_detail<'a>(detail: &'a Value, key: &'a str) -> impl Iterator<Item = &'a Value> {
    detail
        .get("detail")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(move |inst| inst.get(key).and_then(Value::as_array))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // Provider discovery returns the `byInstanceType` *keys* verbatim — they
    // are the case-sensitive `applicationType` values the detail endpoint
    // wants (e.g. `oAuth-mR7e…`), not the human `name`. Lowercasing them (an
    // earlier mistake) made the detail call return zero records.
    #[test]
    fn providers_with_data_returns_instance_keys_verbatim_for_populated_types() {
        let summary = json!({
            "summary": {
                "branch": {
                    "overall": { "count": 8 },
                    "byInstanceType": {
                        "bitbucket": { "count": 4, "name": "Bitbucket Cloud" },
                        "oAuth-mR7eEFbRUZxJL5OS7y4UcJMHO4Azw6lm": { "count": 9, "name": "On-Prem" },
                        "empty": { "count": 0, "name": "Unused" }
                    }
                },
                "build": { "overall": { "count": 0 }, "byInstanceType": {} }
            }
        });
        let mut got = providers_with_data(&summary, "branch");
        got.sort();
        assert_eq!(
            got,
            vec!["bitbucket", "oAuth-mR7eEFbRUZxJL5OS7y4UcJMHO4Azw6lm"]
        );
        assert!(providers_with_data(&summary, "build").is_empty());
        assert!(providers_with_data(&summary, "pullrequest").is_empty());
    }

    #[test]
    fn integration_state_distinguishes_data_empty_and_unavailable() {
        let has_data = json!({
            "errors": [], "configErrors": [],
            "summary": { "pullrequest": { "overall": { "count": 1 } } }
        });
        assert_eq!(integration_state(&has_data), IntegrationState::HasData);

        let empty = json!({ "errors": [], "configErrors": [], "summary": {} });
        assert_eq!(integration_state(&empty), IntegrationState::NoData);

        let zeroed = json!({
            "summary": { "pullrequest": { "overall": { "count": 0 } } }
        });
        assert_eq!(integration_state(&zeroed), IntegrationState::NoData);

        let unavailable = json!({
            "errors": [{ "message": "permission denied" }], "summary": {}
        });
        assert_eq!(
            integration_state(&unavailable),
            IntegrationState::NoIntegration
        );
    }

    #[test]
    fn project_summary_emits_counts_and_state_for_present_types() {
        let summary = json!({
            "summary": {
                "pullrequest": { "overall": { "count": 2, "state": "OPEN" } },
                "branch": { "overall": { "count": 1 } },
                "build": { "overall": { "count": 3, "state": "FAILED" } }
            }
        });
        assert_eq!(
            project_summary(&summary),
            json!({
                "pullrequest": { "count": 2, "state": "OPEN" },
                "branch": { "count": 1 },
                "build": { "count": 3, "state": "FAILED" }
            })
        );
    }

    #[test]
    fn project_pull_requests_flattens_and_curates() {
        let detail = json!({
            "detail": [{
                "pullRequests": [{
                    "id": "#1",
                    "name": "Add dev-status",
                    "url": "https://github.com/o/r/pull/1",
                    "status": "OPEN",
                    "author": { "name": "octocat" },
                    "source": { "branch": "feature/dev-status" },
                    "destination": { "branch": "main" },
                    "lastUpdate": "2026-06-20T10:00:00Z"
                }],
                "instance": { "type": "GitHub" }
            }]
        });
        assert_eq!(
            project_pull_requests(&detail),
            vec![json!({
                "id": "#1",
                "title": "Add dev-status",
                "url": "https://github.com/o/r/pull/1",
                "status": "OPEN",
                "author": "octocat",
                "source_branch": "feature/dev-status",
                "dest_branch": "main",
                "last_update": "2026-06-20T10:00:00Z"
            })]
        );
    }

    #[test]
    fn project_pull_requests_empty_when_no_detail() {
        assert!(project_pull_requests(&json!({ "detail": [] })).is_empty());
    }

    // Real `dataType=branch` shape: branches sit directly at
    // `detail[].branches[]`, each carrying its owning repo under
    // `repository.name` (not nested inside a `repositories[]` array).
    #[test]
    fn project_branches_carries_repo_and_last_commit() {
        let detail = json!({
            "detail": [{
                "branches": [{
                    "name": "feature/dev-status",
                    "url": "https://bitbucket.org/acme/web/branch/feature/dev-status",
                    "repository": { "name": "acme/web" },
                    "lastCommit": {
                        "id": "abc1234",
                        "message": "wire dev-status",
                        "authorTimestamp": "2026-06-20T10:00:00Z"
                    }
                }]
            }]
        });
        assert_eq!(
            project_branches(&detail),
            vec![json!({
                "name": "feature/dev-status",
                "url": "https://bitbucket.org/acme/web/branch/feature/dev-status",
                "repo": "acme/web",
                "last_commit": {
                    "hash": "abc1234",
                    "message": "wire dev-status",
                    "date": "2026-06-20T10:00:00Z"
                }
            })]
        );
    }

    // Both build envelopes must surface: legacy `detail[].builds[]` (with
    // `name`) and modern Forge `detail[].jswddBuildsData[].builds[]` (with
    // `displayName`).
    #[test]
    fn project_builds_merges_legacy_and_modern_envelopes() {
        let detail = json!({
            "detail": [
                {
                    "builds": [{
                        "name": "CI #42",
                        "url": "https://ci.example.com/42",
                        "state": "successful",
                        "lastUpdated": "2026-06-20T11:00:00Z"
                    }]
                },
                {
                    "jswddBuildsData": [{
                        "builds": [{
                            "displayName": "aop-blackbox-daily",
                            "url": "https://jenkins.example.com/job/516",
                            "state": "failed",
                            "lastUpdated": "2026-06-24T09:16:01Z"
                        }]
                    }]
                }
            ]
        });
        assert_eq!(
            project_builds(&detail),
            vec![
                json!({
                    "name": "CI #42",
                    "url": "https://ci.example.com/42",
                    "state": "successful",
                    "last_update": "2026-06-20T11:00:00Z"
                }),
                json!({
                    "name": "aop-blackbox-daily",
                    "url": "https://jenkins.example.com/job/516",
                    "state": "failed",
                    "last_update": "2026-06-24T09:16:01Z"
                })
            ]
        );
    }
}
