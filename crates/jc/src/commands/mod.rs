use std::path::Path;

use jc_core::Client;
use jc_jira::jql::JqlBuilder;
use jc_jira::transitions::{self, MatchResult};
use serde_json::{Value, json};
use similar::TextDiff;

use crate::cli::{
    Cli, Command, ConfigCommand, JiraCommand, JiraCommentCommand, JiraIssueCommand,
};
use crate::config::Config;
use crate::output::{CliError, Envelope};
use crate::preview::{Preview, PreviewMode};

pub async fn dispatch(args: Cli) -> Result<(), CliError> {
    let limit = args.limit;
    let show_query = args.show_query;
    let mode = PreviewMode::from_flags(args.dry_run, args.confirm);

    match args.command {
        Command::Config(ConfigCommand::Show) => config_show(),
        Command::Config(ConfigCommand::Test) => config_test().await,

        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Get { key })) => {
            jira_issue_get(&key).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::List {
            project,
            status,
            assignee,
            issue_type,
            updated,
        })) => {
            let mut b = JqlBuilder::new();
            if let Some(p) = project {
                b = b.eq("project", &p);
            }
            if let Some(s) = status {
                b = b.eq("status", &s);
            }
            if let Some(a) = assignee {
                b = apply_assignee(b, &a);
            }
            if let Some(t) = issue_type {
                b = b.eq("issuetype", &t);
            }
            if let Some(u) = updated {
                b = b.raw(format!("updated >= {u}"));
            }
            b = b.order_by("updated DESC");
            run_jql(&b.build(), limit, show_query).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Mine { status })) => {
            let mut b = JqlBuilder::new().raw("assignee = currentUser()");
            if let Some(s) = status {
                b = b.eq("status", &s);
            }
            b = b.order_by("updated DESC");
            run_jql(&b.build(), limit, show_query).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Search { terms, project })) => {
            let mut b = JqlBuilder::new().contains("summary", &terms);
            if let Some(p) = project {
                b = b.eq("project", &p);
            }
            b = b.order_by("updated DESC");
            run_jql(&b.build(), limit, show_query).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Comment(
            JiraCommentCommand::Add { key, body_file },
        ))) => jira_comment_add(&key, &body_file, mode).await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Comment(
            JiraCommentCommand::List { key },
        ))) => jira_comment_list(&key, limit).await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Comment(
            JiraCommentCommand::Edit {
                key,
                comment_id,
                body_file,
            },
        ))) => jira_comment_edit(&key, &comment_id, &body_file, mode).await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Comment(
            JiraCommentCommand::Delete { key, comment_id },
        ))) => jira_comment_delete(&key, &comment_id, mode).await,

        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Transition { key, to })) => {
            jira_issue_transition(&key, &to, mode).await
        }

        Command::Jira(JiraCommand::Jql { query }) => run_jql(&query, limit, show_query).await,

        Command::Conf(_) => {
            unreachable!("conf subcommands not yet implemented")
        }
    }
}

fn apply_assignee(b: JqlBuilder, who: &str) -> JqlBuilder {
    if who.eq_ignore_ascii_case("me") || who.eq_ignore_ascii_case("currentuser") {
        b.raw("assignee = currentUser()")
    } else {
        b.eq("assignee", who)
    }
}

fn config_show() -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    Envelope::new(cfg.redacted_json()).emit();
    Ok(())
}

async fn config_test() -> Result<(), CliError> {
    let client = jira_client()?;
    let me = jc_jira::users::myself(&client).await?;
    Envelope::new(json!({
        "ok": true,
        "account_id": me.account_id,
        "display_name": me.display_name,
        "email": me.email_address,
        "active": me.active,
    }))
    .emit();
    Ok(())
}

async fn jira_issue_get(key: &str) -> Result<(), CliError> {
    let client = jira_client()?;
    let issue = jc_jira::issue::get(&client, key).await?;

    let description_markdown = issue
        .fields
        .description
        .as_ref()
        .map(jc_adf::to_markdown);

    let data = json!({
        "id": issue.id,
        "key": issue.key,
        "summary": issue.fields.summary,
        "issue_type": issue.fields.issuetype.as_ref().map(|t| &t.name),
        "status": issue.fields.status.as_ref().map(|s| &s.name),
        "status_category": issue.fields.status.as_ref().and_then(|s| s.category.as_ref().map(|c| &c.key)),
        "priority": issue.fields.priority.as_ref().map(|p| &p.name),
        "assignee": issue.fields.assignee.as_ref().map(|u| json!({
            "account_id": u.account_id,
            "display_name": u.display_name,
        })),
        "reporter": issue.fields.reporter.as_ref().map(|u| json!({
            "account_id": u.account_id,
            "display_name": u.display_name,
        })),
        "labels": issue.fields.labels,
        "description_markdown": description_markdown,
        "comments": {
            "count": issue.fields.comment.as_ref().map(|c| c.total).unwrap_or(0),
        },
        "attachments": issue.fields.attachment.iter().map(|a| json!({
            "id": a.id,
            "filename": a.filename,
            "mime_type": a.mime_type,
            "size": a.size,
        })).collect::<Vec<_>>(),
    });

    let mut env = Envelope::new(data);
    let non_inline = issue.fields.attachment.len();
    if non_inline > 0 {
        env.warnings.push(format!(
            "{non_inline} attachment(s) available — use `jc jira issue attachment get <id>` to download"
        ));
    }
    env.emit();
    Ok(())
}

async fn run_jql(query: &str, limit: usize, show_query: bool) -> Result<(), CliError> {
    let client = jira_client()?;
    let hits = jc_jira::search::jql(&client, query, jc_jira::search::DEFAULT_FIELDS, limit).await?;

    let issues: Vec<_> = hits
        .iter()
        .map(|h| {
            json!({
                "key": h.key,
                "summary": h.fields.summary,
                "status": h.fields.status.as_ref().map(|s| &s.name),
                "assignee": h.fields.assignee.as_ref().map(|u| &u.display_name),
                "priority": h.fields.priority.as_ref().map(|p| &p.name),
                "issue_type": h.fields.issuetype.as_ref().map(|t| &t.name),
                "updated": h.fields.updated,
                "labels": h.fields.labels,
            })
        })
        .collect();

    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(hits.len()));
    if show_query {
        meta.insert("query".into(), json!(query));
    }

    let mut env = Envelope::new(issues);
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn jira_comment_add(
    key: &str,
    body_file: &Path,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let md = std::fs::read_to_string(body_file).map_err(|e| {
        CliError::io(format!("read {}: {e}", body_file.display()))
    })?;
    if md.trim().is_empty() {
        return Err(CliError::validation(format!(
            "body file {} is empty",
            body_file.display()
        )));
    }
    let adf = jc_adf::to_adf(&md);

    let cfg = Config::from_env()?;
    let url = format!("https://{}/rest/api/3/issue/{}/comment", cfg.site, key);
    let preview = Preview::new("POST", url)
        .with_body(json!({ "body": adf }))
        .with_summary(format!("Add comment to {key}"));

    match mode {
        PreviewMode::DryRun => {
            preview.emit_dry_run();
            return Ok(());
        }
        PreviewMode::Confirm => {
            if !preview.confirm_interactive()? {
                let mut env = Envelope::new(json!({ "cancelled": true }));
                let mut meta = serde_json::Map::new();
                meta.insert("mode".into(), json!("confirm"));
                env.meta = Some(Value::Object(meta));
                env.emit();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    let client = cfg.jira_client()?;
    let comment = jc_jira::comment::add(&client, key, &adf).await?;
    Envelope::new(json!({
        "id": comment.id,
        "created": comment.created,
        "author": comment.author.as_ref().map(|u| &u.display_name),
    }))
    .emit();
    Ok(())
}

async fn jira_comment_list(key: &str, limit: usize) -> Result<(), CliError> {
    let client = jira_client()?;
    let comments = jc_jira::comment::list(&client, key, limit).await?;

    let data: Vec<_> = comments
        .iter()
        .map(|c| {
            json!({
                "id": c.id,
                "author": c.author.as_ref().map(|u| &u.display_name),
                "created": c.created,
                "updated": c.updated,
                "body_markdown": c.body.as_ref().map(jc_adf::to_markdown),
            })
        })
        .collect();

    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(comments.len()));
    meta.insert("issue".into(), json!(key));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn jira_comment_edit(
    key: &str,
    comment_id: &str,
    body_file: &Path,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let md = std::fs::read_to_string(body_file)
        .map_err(|e| CliError::io(format!("read {}: {e}", body_file.display())))?;
    if md.trim().is_empty() {
        return Err(CliError::validation(format!(
            "body file {} is empty",
            body_file.display()
        )));
    }
    let new_adf = jc_adf::to_adf(&md);
    let new_markdown = jc_adf::to_markdown(&new_adf);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;

    // Fetch current state for the diff preview.
    let current = jc_jira::comment::get(&client, key, comment_id).await?;
    let current_markdown = current
        .body
        .as_ref()
        .map(jc_adf::to_markdown)
        .unwrap_or_default();

    let diff = unified_diff(&current_markdown, &new_markdown);

    let url = format!(
        "https://{}/rest/api/3/issue/{}/comment/{}",
        cfg.site, key, comment_id
    );
    let preview = Preview::new("PUT", url)
        .with_body(json!({ "body": new_adf }))
        .with_summary(format!("Edit comment {comment_id} on {key}"))
        .with_diff(diff);

    match mode {
        PreviewMode::DryRun => {
            preview.emit_dry_run();
            return Ok(());
        }
        PreviewMode::Confirm => {
            if !preview.confirm_interactive()? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    let updated = jc_jira::comment::edit(&client, key, comment_id, &new_adf).await?;
    Envelope::new(json!({
        "id": updated.id,
        "updated": updated.updated,
        "author": updated.author.as_ref().map(|u| &u.display_name),
    }))
    .emit();
    Ok(())
}

async fn jira_comment_delete(
    key: &str,
    comment_id: &str,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let url = format!(
        "https://{}/rest/api/3/issue/{}/comment/{}",
        cfg.site, key, comment_id
    );
    let preview = Preview::new("DELETE", url)
        .with_summary(format!("Delete comment {comment_id} on {key}"));

    match mode {
        PreviewMode::DryRun => {
            preview.emit_dry_run();
            return Ok(());
        }
        PreviewMode::Confirm => {
            if !preview.confirm_interactive()? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    let client = cfg.jira_client()?;
    jc_jira::comment::delete(&client, key, comment_id).await?;
    Envelope::new(json!({
        "deleted": true,
        "id": comment_id,
        "issue": key,
    }))
    .emit();
    Ok(())
}

async fn jira_issue_transition(
    key: &str,
    target: &str,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;

    let available = transitions::list(&client, key).await?;
    let matched = match transitions::find_match(&available, target) {
        MatchResult::Unique(t) => t,
        MatchResult::Ambiguous(cands) => {
            let names: Vec<&str> = cands.iter().map(|t| t.name.as_str()).collect();
            return Err(CliError::validation(format!(
                "transition '{target}' is ambiguous: {}",
                names.join(", ")
            )));
        }
        MatchResult::NotFound => {
            let names: Vec<&str> = available.iter().map(|t| t.name.as_str()).collect();
            return Err(CliError::validation(format!(
                "no transition matches '{target}'. available: {}",
                if names.is_empty() {
                    "(none)".to_string()
                } else {
                    names.join(", ")
                }
            )));
        }
    };

    let url = format!("https://{}/rest/api/3/issue/{}/transitions", cfg.site, key);
    let preview = Preview::new("POST", url)
        .with_body(json!({
            "transition": { "id": matched.id, "name": matched.name }
        }))
        .with_summary(format!("Transition {key} -> {}", matched.name));

    match mode {
        PreviewMode::DryRun => {
            preview.emit_dry_run();
            return Ok(());
        }
        PreviewMode::Confirm => {
            if !preview.confirm_interactive()? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    transitions::execute(&client, key, &matched.id).await?;
    Envelope::new(json!({
        "transitioned": true,
        "issue": key,
        "transition_id": matched.id,
        "to": matched.name,
        "to_status": matched.to.as_ref().map(|t| &t.name),
    }))
    .emit();
    Ok(())
}

fn emit_cancelled() {
    let mut env = Envelope::new(json!({ "cancelled": true }));
    let mut meta = serde_json::Map::new();
    meta.insert("mode".into(), json!("confirm"));
    env.meta = Some(Value::Object(meta));
    env.emit();
}

fn unified_diff(before: &str, after: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header("before", "after")
        .to_string()
}

fn jira_client() -> Result<Client, CliError> {
    Ok(Config::from_env()?.jira_client()?)
}
