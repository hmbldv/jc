//! Workflow transitions.
//!
//! Transitions are workflow-specific and keyed by numeric ID. To keep the
//! UX sane we let users pass a name (`--to "In Review"`) and resolve it
//! against `/rest/api/3/issue/{key}/transitions` with a fuzzy matcher:
//! exact match first, case-insensitive contains as fallback, ambiguous
//! and not-found cases error out with the candidate list.

use jc_core::{Client, Result};
use reqwest::Method;
use serde::{Deserialize, Serialize};

use crate::issue::StatusCategory;

#[derive(Debug, Deserialize)]
pub struct Transition {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub to: Option<TransitionTo>,
    #[serde(rename = "isAvailable", default = "default_true")]
    pub is_available: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
pub struct TransitionTo {
    pub name: String,
    #[serde(rename = "statusCategory", default)]
    pub category: Option<StatusCategory>,
}

#[derive(Debug, Deserialize)]
struct TransitionsResponse {
    #[serde(default)]
    transitions: Vec<Transition>,
}

#[derive(Debug, Serialize)]
struct TransitionRequest<'a> {
    transition: TransitionRef<'a>,
}

#[derive(Debug, Serialize)]
struct TransitionRef<'a> {
    id: &'a str,
}

/// GET /rest/api/3/issue/{key}/transitions
pub async fn list(client: &Client, issue_key: &str) -> Result<Vec<Transition>> {
    let path = format!("rest/api/3/issue/{issue_key}/transitions");
    let resp: TransitionsResponse = client.request_json(Method::GET, &path).await?;
    Ok(resp.transitions)
}

/// POST /rest/api/3/issue/{key}/transitions
pub async fn execute(client: &Client, issue_key: &str, transition_id: &str) -> Result<()> {
    let path = format!("rest/api/3/issue/{issue_key}/transitions");
    let req = TransitionRequest {
        transition: TransitionRef { id: transition_id },
    };
    client.post_no_content(&path, &req).await
}

#[derive(Debug)]
pub enum MatchResult<'a> {
    Unique(&'a Transition),
    Ambiguous(Vec<&'a Transition>),
    NotFound,
}

/// Fuzzy-match a user-supplied name against a list of transitions.
///
/// Strategy:
/// 1. Case-insensitive exact match. Unique winner wins; multiple matches
///    fall through to step 2.
/// 2. Case-insensitive substring match. Unique winner wins; multiple
///    matches are ambiguous; zero matches is NotFound.
pub fn find_match<'a>(transitions: &'a [Transition], target: &str) -> MatchResult<'a> {
    let norm = target.trim().to_ascii_lowercase();
    if norm.is_empty() {
        return MatchResult::NotFound;
    }

    let exact: Vec<&Transition> = transitions
        .iter()
        .filter(|t| t.name.to_ascii_lowercase() == norm)
        .collect();
    match exact.len() {
        1 => return MatchResult::Unique(exact[0]),
        n if n > 1 => return MatchResult::Ambiguous(exact),
        _ => {}
    }

    let contains: Vec<&Transition> = transitions
        .iter()
        .filter(|t| t.name.to_ascii_lowercase().contains(&norm))
        .collect();
    match contains.len() {
        0 => MatchResult::NotFound,
        1 => MatchResult::Unique(contains[0]),
        _ => MatchResult::Ambiguous(contains),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, name: &str) -> Transition {
        Transition {
            id: id.to_string(),
            name: name.to_string(),
            to: None,
            is_available: true,
        }
    }

    #[test]
    fn unique_exact() {
        let ts = vec![t("11", "To Do"), t("21", "In Progress"), t("31", "Done")];
        match find_match(&ts, "Done") {
            MatchResult::Unique(m) => assert_eq!(m.id, "31"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn exact_case_insensitive() {
        let ts = vec![t("31", "Done")];
        match find_match(&ts, "done") {
            MatchResult::Unique(m) => assert_eq!(m.id, "31"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn exact_wins_over_contains() {
        // "Done" is an exact match; "Work Done" would also be a substring
        // match but exact should win.
        let ts = vec![t("31", "Done"), t("41", "Work Done")];
        match find_match(&ts, "done") {
            MatchResult::Unique(m) => assert_eq!(m.id, "31"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn contains_unique() {
        let ts = vec![t("21", "In Progress"), t("31", "Done")];
        match find_match(&ts, "progress") {
            MatchResult::Unique(m) => assert_eq!(m.id, "21"),
            other => panic!("expected Unique, got {other:?}"),
        }
    }

    #[test]
    fn contains_ambiguous() {
        let ts = vec![t("41", "Work Done"), t("42", "Task Done")];
        match find_match(&ts, "done") {
            MatchResult::Ambiguous(cands) => assert_eq!(cands.len(), 2),
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn not_found() {
        let ts = vec![t("11", "To Do"), t("21", "In Progress")];
        assert!(matches!(find_match(&ts, "xyz"), MatchResult::NotFound));
    }

    #[test]
    fn empty_target_not_found() {
        let ts = vec![t("11", "To Do")];
        assert!(matches!(find_match(&ts, "   "), MatchResult::NotFound));
    }
}
