//! Markdown mention pre-processor.
//!
//! Users write `@[query]` in markdown to mean "this is a real user
//! mention, resolve it." The query is whatever the user typed —
//! a full accountId, an email, or a display name substring.
//!
//! Resolution is two-phase:
//!
//! 1. [`find_mention_queries`] scans the markdown source and returns
//!    the unique set of queries to resolve (sync, no I/O).
//! 2. [`resolve_mentions`] hits the Jira user search endpoint to turn
//!    each query into a real `(accountId, displayName)` pair. Exact
//!    display-name or email match wins; a single partial match is
//!    accepted; multiple partial matches without a tiebreaker error
//!    with the candidate list.
//!
//! After the queries are resolved, [`rewrite_mentions`] produces a
//! normalized markdown string where every `@[query]` is replaced with
//! `@[accountId]`, and [`apply_mentions_to_adf`] walks the ADF doc
//! produced by `jc_adf::to_adf` and splits each unmarked text node
//! on its `@[accountId]` tokens, emitting proper ADF `mention` inline
//! nodes alongside the surrounding text fragments.
//!
//! The reason the post-processor runs on the ADF tree rather than the
//! markdown source is that a mention inline node can't be expressed in
//! CommonMark's grammar — it's a first-class ADF node type, not
//! styled text — so we produce it after the markdown parse has
//! finished, touching only unmarked text nodes to avoid dropping any
//! formatting the user might have wrapped around a mention.

use std::collections::{BTreeMap, BTreeSet};

use jc_core::Client;
use serde_json::{Value, json};

use crate::output::CliError;

#[derive(Debug, Clone)]
pub struct ResolvedMention {
    pub account_id: String,
    pub display_name: String,
}

/// Scan `md` for `@[query]` tokens. Returns each unique query once
/// (order preserved by first appearance) so the caller hits the
/// user-search API once per distinct user.
pub fn find_mention_queries(md: &str) -> Vec<String> {
    let bytes = md.as_bytes();
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'@' && bytes[i + 1] == b'[' {
            let content_start = i + 2;
            if let Some(offset) = bytes[content_start..].iter().position(|b| *b == b']') {
                let query = &md[content_start..content_start + offset];
                let trimmed = query.trim();
                if !trimmed.is_empty() && seen.insert(trimmed.to_string()) {
                    out.push(trimmed.to_string());
                }
                i = content_start + offset + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Resolve each query via Jira user search. An accountId-shaped query
/// is used directly without a lookup so Claude Code (or scripts) can
/// skip the search round-trip when they already know the ID.
pub async fn resolve_mentions(
    client: &Client,
    queries: &[String],
) -> Result<BTreeMap<String, ResolvedMention>, CliError> {
    let mut out = BTreeMap::new();
    for query in queries {
        let resolved = resolve_one(client, query).await?;
        out.insert(query.clone(), resolved);
    }
    Ok(out)
}

async fn resolve_one(client: &Client, query: &str) -> Result<ResolvedMention, CliError> {
    let trimmed = query.trim();
    if looks_like_account_id(trimmed) {
        // The user gave us a raw accountId — trust it and use the
        // accountId as the display name fallback. The display name
        // only affects the rendered `@text` inside the mention node,
        // not the notification target.
        return Ok(ResolvedMention {
            account_id: trimmed.to_string(),
            display_name: trimmed.to_string(),
        });
    }

    let users = jc_jira::users::search(client, trimmed, 10).await?;
    if users.is_empty() {
        return Err(CliError::validation(format!(
            "no user matches mention '@[{query}]'"
        )));
    }

    // Prefer an exact case-insensitive match on display name or email.
    let exact = users.iter().find(|u| {
        u.display_name.eq_ignore_ascii_case(trimmed)
            || u.email_address
                .as_deref()
                .map(|e| e.eq_ignore_ascii_case(trimmed))
                .unwrap_or(false)
    });
    if let Some(u) = exact {
        return Ok(ResolvedMention {
            account_id: u.account_id.clone(),
            display_name: u.display_name.clone(),
        });
    }

    if users.len() == 1 {
        let u = users.into_iter().next().unwrap();
        return Ok(ResolvedMention {
            account_id: u.account_id,
            display_name: u.display_name,
        });
    }

    let names: Vec<&str> = users.iter().map(|u| u.display_name.as_str()).collect();
    Err(CliError::validation(format!(
        "mention '@[{query}]' is ambiguous ({} matches): {}. Use @[<accountId>] or be more specific.",
        users.len(),
        names.join(", ")
    )))
}

/// Rough shape of an Atlassian Cloud accountId. Long alphanumeric with
/// optional `-` / `:`, no spaces, no `@`. Good enough to skip a
/// round-trip when the input is clearly an ID, not a name.
fn looks_like_account_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() >= 16
        && !s.contains(' ')
        && !s.contains('@')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == ':')
}

/// Rewrite `@[query]` tokens to `@[accountId]`. Queries that weren't
/// resolved (shouldn't happen after a successful `resolve_mentions`)
/// are left in place.
pub fn rewrite_mentions(md: &str, resolved: &BTreeMap<String, ResolvedMention>) -> String {
    let mut out = md.to_string();
    for (query, mention) in resolved {
        let needle = format!("@[{query}]");
        let replacement = format!("@[{}]", mention.account_id);
        if needle != replacement {
            out = out.replace(&needle, &replacement);
        }
    }
    out
}

/// Walk an ADF document and replace `@[accountId]` tokens inside
/// unmarked text nodes with proper ADF `mention` inline nodes.
///
/// Text nodes that carry marks (bold, italic, code, etc.) are left
/// alone — mention nodes don't support marks in ADF, so splitting a
/// marked text run would drop formatting. Mentions inside formatted
/// text fall back to literal `@[accountId]` display.
pub fn apply_mentions_to_adf(adf: &mut Value, resolved: &BTreeMap<String, ResolvedMention>) {
    let display_by_id: BTreeMap<&str, &str> = resolved
        .values()
        .map(|m| (m.account_id.as_str(), m.display_name.as_str()))
        .collect();
    walk_node(adf, &display_by_id);
}

fn walk_node(node: &mut Value, display: &BTreeMap<&str, &str>) {
    if let Some(array) = node.get_mut("content").and_then(|v| v.as_array_mut()) {
        let original = std::mem::take(array);
        let mut rewritten = Vec::with_capacity(original.len());
        for mut child in original {
            if is_plain_text(&child) {
                let text = child
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let pieces = split_text(&text, display);
                rewritten.extend(pieces);
            } else {
                walk_node(&mut child, display);
                rewritten.push(child);
            }
        }
        *array = rewritten;
    }
}

fn is_plain_text(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("text") && node.get("marks").is_none()
}

/// Split a plain-text run into alternating text + mention pieces.
/// Only accountIds that are present in `display` are converted;
/// unknown `@[...]` tokens pass through as literal text so we never
/// silently drop a mention.
fn split_text(text: &str, display: &BTreeMap<&str, &str>) -> Vec<Value> {
    let bytes = text.as_bytes();
    let mut out: Vec<Value> = Vec::new();
    let mut cursor = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'@' && bytes[i + 1] == b'[' {
            let start = i + 2;
            if let Some(end_offset) = bytes[start..].iter().position(|b| *b == b']') {
                let account_id = &text[start..start + end_offset];
                if let Some(display_name) = display.get(account_id) {
                    if cursor < i {
                        out.push(json!({
                            "type": "text",
                            "text": &text[cursor..i],
                        }));
                    }
                    out.push(json!({
                        "type": "mention",
                        "attrs": {
                            "id": account_id,
                            "text": format!("@{display_name}"),
                        }
                    }));
                    i = start + end_offset + 1;
                    cursor = i;
                    continue;
                }
            }
        }
        i += 1;
    }
    if cursor < text.len() {
        out.push(json!({
            "type": "text",
            "text": &text[cursor..],
        }));
    }
    if out.is_empty() {
        // No mention matched; return the whole text as one node so we
        // never strip content accidentally.
        out.push(json!({"type": "text", "text": text}));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_resolved() -> BTreeMap<String, ResolvedMention> {
        let mut m = BTreeMap::new();
        m.insert(
            "alice".to_string(),
            ResolvedMention {
                account_id: "acct-alice".to_string(),
                display_name: "Alice Smith".to_string(),
            },
        );
        m.insert(
            "bob".to_string(),
            ResolvedMention {
                account_id: "acct-bob".to_string(),
                display_name: "Bob Jones".to_string(),
            },
        );
        m
    }

    #[test]
    fn find_single_mention() {
        let md = "Hello @[alice], please review.";
        assert_eq!(find_mention_queries(md), vec!["alice".to_string()]);
    }

    #[test]
    fn find_multiple_unique() {
        let md = "cc @[alice] and @[bob]";
        let mut qs = find_mention_queries(md);
        qs.sort();
        assert_eq!(qs, vec!["alice".to_string(), "bob".to_string()]);
    }

    #[test]
    fn find_dedupes() {
        let md = "@[alice] and @[alice] again";
        assert_eq!(find_mention_queries(md), vec!["alice".to_string()]);
    }

    #[test]
    fn find_skips_unclosed_bracket() {
        let md = "@[alice";
        assert!(find_mention_queries(md).is_empty());
    }

    #[test]
    fn find_skips_bare_at_sign() {
        let md = "@alice without brackets";
        assert!(find_mention_queries(md).is_empty());
    }

    #[test]
    fn find_trims_whitespace() {
        let md = "@[  alice  ]";
        assert_eq!(find_mention_queries(md), vec!["alice".to_string()]);
    }

    #[test]
    fn account_id_heuristic() {
        assert!(looks_like_account_id("5b10a2844c20165700ede21g"));
        assert!(looks_like_account_id("712020:abc-def-ghi"));
        assert!(!looks_like_account_id("alice"));
        assert!(!looks_like_account_id("alice@example.com"));
        assert!(!looks_like_account_id("Alice Smith"));
        assert!(!looks_like_account_id("short"));
    }

    #[test]
    fn rewrite_replaces_query_with_account_id() {
        let md = "cc @[alice]";
        let out = rewrite_mentions(md, &sample_resolved());
        assert_eq!(out, "cc @[acct-alice]");
    }

    #[test]
    fn rewrite_handles_multiple_queries() {
        let md = "@[alice] and @[bob]";
        let out = rewrite_mentions(md, &sample_resolved());
        assert!(out.contains("@[acct-alice]"));
        assert!(out.contains("@[acct-bob]"));
    }

    #[test]
    fn split_text_no_mentions() {
        let display = BTreeMap::new();
        let pieces = split_text("just a sentence", &display);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0]["text"], "just a sentence");
    }

    #[test]
    fn split_text_mention_in_middle() {
        let mut display = BTreeMap::new();
        display.insert("acct-alice", "Alice Smith");
        let pieces = split_text("cc @[acct-alice] please", &display);
        assert_eq!(pieces.len(), 3);
        assert_eq!(pieces[0]["text"], "cc ");
        assert_eq!(pieces[1]["type"], "mention");
        assert_eq!(pieces[1]["attrs"]["id"], "acct-alice");
        assert_eq!(pieces[1]["attrs"]["text"], "@Alice Smith");
        assert_eq!(pieces[2]["text"], " please");
    }

    #[test]
    fn split_text_unknown_account_id_passthrough() {
        // If the accountId isn't in the map, leave the literal @[...]
        // in place so the user still sees what they wrote.
        let display = BTreeMap::new();
        let pieces = split_text("cc @[unknown]", &display);
        assert_eq!(pieces.len(), 1);
        assert_eq!(pieces[0]["text"], "cc @[unknown]");
    }

    #[test]
    fn apply_mentions_to_adf_document() {
        let mut adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "cc @[acct-alice] for review"
                }]
            }]
        });
        let mut resolved = BTreeMap::new();
        resolved.insert(
            "alice".to_string(),
            ResolvedMention {
                account_id: "acct-alice".to_string(),
                display_name: "Alice Smith".to_string(),
            },
        );
        apply_mentions_to_adf(&mut adf, &resolved);

        let para_content = adf["content"][0]["content"].as_array().unwrap();
        assert_eq!(para_content.len(), 3);
        assert_eq!(para_content[0]["text"], "cc ");
        assert_eq!(para_content[1]["type"], "mention");
        assert_eq!(para_content[1]["attrs"]["id"], "acct-alice");
        assert_eq!(para_content[2]["text"], " for review");
    }

    #[test]
    fn apply_mentions_skips_marked_text() {
        // A mention inside **bold** lives in a marked text node, which
        // we intentionally leave alone so formatting isn't dropped.
        let mut adf = json!({
            "type": "doc",
            "version": 1,
            "content": [{
                "type": "paragraph",
                "content": [{
                    "type": "text",
                    "text": "@[acct-alice] wrote this",
                    "marks": [{"type": "strong"}]
                }]
            }]
        });
        let mut resolved = BTreeMap::new();
        resolved.insert(
            "alice".to_string(),
            ResolvedMention {
                account_id: "acct-alice".to_string(),
                display_name: "Alice Smith".to_string(),
            },
        );
        apply_mentions_to_adf(&mut adf, &resolved);
        // Unchanged — the marked text node doesn't get split.
        let node = &adf["content"][0]["content"][0];
        assert_eq!(node["type"], "text");
        assert_eq!(node["text"], "@[acct-alice] wrote this");
    }
}
