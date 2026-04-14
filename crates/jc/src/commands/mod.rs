use std::path::Path;

use jc_core::Client;
use jc_jira::jql::JqlBuilder;
use jc_jira::transitions::{self, MatchResult};
use serde_json::{Value, json};
use similar::TextDiff;

use crate::cli::{
    Cli, Command, ConfCommand, ConfPageCommand, ConfSpaceCommand, ConfigCommand,
    FieldsSubcommand, JiraAttachmentCommand, JiraCommand, JiraCommentCommand, JiraIssueCommand,
};
use crate::config::Config;
use crate::output::{CliError, Envelope};
use crate::preview::{Preview, PreviewMode, emit_composite_dry_run, prompt_yes_no};

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
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Create {
            project,
            issue_type,
            summary,
            description_file,
            fields,
        })) => {
            jira_issue_create(
                &project,
                &issue_type,
                &summary,
                description_file.as_deref(),
                &fields,
                mode,
            )
            .await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Edit {
            key,
            summary,
            description_file,
            fields,
        })) => {
            jira_issue_edit(
                &key,
                summary.as_deref(),
                description_file.as_deref(),
                &fields,
                mode,
            )
            .await
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
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Attachment(
            JiraAttachmentCommand::List { key },
        ))) => jira_attachment_list(&key).await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Attachment(
            JiraAttachmentCommand::Get { id, out_dir },
        ))) => jira_attachment_get(&id, &out_dir).await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Attachment(
            JiraAttachmentCommand::Upload { key, file },
        ))) => jira_attachment_upload(&key, &file, mode).await,

        Command::Jira(JiraCommand::Jql { query }) => run_jql(&query, limit, show_query).await,
        Command::Jira(JiraCommand::Fields(fields_cmd)) => match fields_cmd.command {
            FieldsSubcommand::Sync => jira_fields_sync().await,
        },

        Command::Conf(ConfCommand::Page(ConfPageCommand::Get { id })) => {
            conf_page_get(&id).await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::List { space, parent })) => {
            conf_page_list(&space, parent.as_deref(), limit).await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::Search { terms, space })) => {
            conf_page_search(&terms, space.as_deref(), limit, show_query).await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::Create {
            space,
            title,
            from_markdown,
            parent,
        })) => {
            conf_page_create(&space, &title, &from_markdown, parent.as_deref(), mode).await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::Update {
            id,
            from_markdown,
            title,
            expected_version,
        })) => {
            conf_page_update(&id, &from_markdown, title.as_deref(), expected_version, mode).await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::Delete { id })) => {
            conf_page_delete(&id, mode).await
        }
        Command::Conf(ConfCommand::Space(ConfSpaceCommand::List)) => conf_space_list().await,
        Command::Conf(ConfCommand::Space(ConfSpaceCommand::Get { key_or_id })) => {
            conf_space_get(&key_or_id).await
        }
        Command::Conf(ConfCommand::Cql { query }) => {
            run_cql(&query, limit, show_query).await
        }

        Command::Publish {
            file,
            space,
            title,
            parent,
            link_to,
        } => {
            publish(
                &file,
                &space,
                &title,
                parent.as_deref(),
                link_to.as_deref(),
                mode,
            )
            .await
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

async fn jira_attachment_list(key: &str) -> Result<(), CliError> {
    let client = jira_client()?;
    let issue = jc_jira::issue::get(&client, key).await?;

    let data: Vec<_> = issue
        .fields
        .attachment
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "filename": a.filename,
                "mime_type": a.mime_type,
                "size": a.size,
            })
        })
        .collect();

    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(issue.fields.attachment.len()));
    meta.insert("issue".into(), json!(key));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn jira_attachment_get(id: &str, out_dir: &Path) -> Result<(), CliError> {
    let client = jira_client()?;
    let meta = jc_jira::attachments::get_meta(&client, id).await?;
    let blob = jc_jira::attachments::download(&client, id).await?;

    std::fs::create_dir_all(out_dir)
        .map_err(|e| CliError::io(format!("mkdir {}: {e}", out_dir.display())))?;
    let path = out_dir.join(&meta.filename);
    std::fs::write(&path, &blob.bytes)
        .map_err(|e| CliError::io(format!("write {}: {e}", path.display())))?;

    let mime = blob.content_type.clone().or(meta.mime_type.clone());
    let warning = unreadable_mime_warning(mime.as_deref());

    let mut env = Envelope::new(json!({
        "id": meta.id,
        "filename": meta.filename,
        "path": path.display().to_string(),
        "size": blob.bytes.len(),
        "mime": mime,
    }));
    if let Some(w) = warning {
        env.warnings.push(w);
    }
    env.emit();
    Ok(())
}

fn unreadable_mime_warning(mime: Option<&str>) -> Option<String> {
    let Some(mime) = mime else {
        return Some("no mime type reported by server".to_string());
    };
    let lower = mime.to_ascii_lowercase();
    let directly_readable = lower.starts_with("text/")
        || lower.starts_with("image/")
        || lower == "application/pdf"
        || lower == "application/json"
        || lower.contains("javascript")
        || lower.contains("xml")
        || lower.contains("yaml")
        || lower.contains("markdown");
    if directly_readable {
        None
    } else {
        Some(format!(
            "mime type '{mime}' may not be directly readable by Claude Code — use a dedicated parser"
        ))
    }
}

async fn jira_attachment_upload(
    key: &str,
    file: &Path,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let bytes = std::fs::read(file)
        .map_err(|e| CliError::io(format!("read {}: {e}", file.display())))?;
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::validation(format!("invalid filename: {}", file.display())))?
        .to_string();
    let mime = guess_mime_from_ext(file);

    let cfg = Config::from_env()?;
    let url = format!("https://{}/rest/api/3/issue/{}/attachments", cfg.site, key);
    let preview = Preview::new("POST", url).with_summary(format!(
        "Upload {filename} ({} bytes) to {key}",
        bytes.len()
    ));

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
    let uploaded =
        jc_jira::attachments::upload(&client, key, &filename, bytes, mime).await?;

    let data: Vec<_> = uploaded
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "filename": a.filename,
                "size": a.size,
                "mime_type": a.mime_type,
            })
        })
        .collect();

    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(uploaded.len()));
    meta.insert("issue".into(), json!(key));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

fn guess_mime_from_ext(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "txt" | "log" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "xml" => "application/xml",
        "yaml" | "yml" => "application/yaml",
        "zip" => "application/zip",
        "tar" => "application/x-tar",
        "gz" => "application/gzip",
        "html" | "htm" => "text/html",
        "csv" => "text/csv",
        _ => return None,
    })
}

async fn conf_page_get(id: &str) -> Result<(), CliError> {
    let client = jira_client()?;
    let page = jc_conf::page::get(&client, id).await?;
    let adf = page.body.as_ref().and_then(|b| b.as_adf());
    let markdown = adf.as_ref().map(jc_adf::to_markdown);

    Envelope::new(json!({
        "id": page.id,
        "title": page.title,
        "space_id": page.space_id,
        "parent_id": page.parent_id,
        "status": page.status,
        "version": page.version.as_ref().map(|v| json!({
            "number": v.number,
            "created_at": v.created_at,
        })),
        "body_markdown": markdown,
    }))
    .emit();
    Ok(())
}

async fn conf_page_list(
    space_key: &str,
    parent: Option<&str>,
    limit: usize,
) -> Result<(), CliError> {
    let client = jira_client()?;
    let space_id = jc_conf::space::resolve_id(&client, space_key).await?;
    let pages = jc_conf::page::list(&client, &space_id, parent, limit).await?;

    let data: Vec<_> = pages
        .iter()
        .map(|p| {
            json!({
                "id": p.id,
                "title": p.title,
                "parent_id": p.parent_id,
                "status": p.status,
                "created_at": p.created_at,
            })
        })
        .collect();

    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(pages.len()));
    meta.insert("space".into(), json!(space_key));
    meta.insert("space_id".into(), json!(space_id));
    if let Some(p) = parent {
        meta.insert("parent".into(), json!(p));
    }
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn conf_page_search(
    terms: &str,
    space: Option<&str>,
    limit: usize,
    show_query: bool,
) -> Result<(), CliError> {
    // Compile a small CQL expression from --terms and --space.
    // CQL string literals use double quotes; escape any embedded quotes.
    let escaped = terms.replace('"', "\\\"");
    let mut query = format!("type = \"page\" AND text ~ \"{escaped}\"");
    if let Some(s) = space {
        query.push_str(&format!(" AND space = \"{}\"", s.replace('"', "\\\"")));
    }
    run_cql(&query, limit, show_query).await
}

async fn conf_page_create(
    space_key: &str,
    title: &str,
    body_file: &Path,
    parent: Option<&str>,
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
    let adf = jc_adf::to_adf(&md);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let space_id = jc_conf::space::resolve_id(&client, space_key).await?;

    let req = jc_conf::page::CreatePageRequest {
        space_id: &space_id,
        status: "current",
        title,
        parent_id: parent,
        body: jc_conf::page::BodyRequest::from_adf(&adf),
    };

    let url = format!("https://{}/wiki/api/v2/pages", cfg.site);
    let preview = Preview::new("POST", url)
        .with_body(serde_json::to_value(&req).unwrap_or_else(|_| json!(null)))
        .with_summary(format!("Create page '{title}' in space {space_key}"));

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

    let page = jc_conf::page::create(&client, &req).await?;
    Envelope::new(json!({
        "id": page.id,
        "title": page.title,
        "space_id": page.space_id,
        "parent_id": page.parent_id,
        "version": page.version.as_ref().map(|v| v.number),
    }))
    .emit();
    Ok(())
}

async fn conf_page_update(
    id: &str,
    body_file: &Path,
    new_title: Option<&str>,
    expected_version: Option<u64>,
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

    // Fetch current page to obtain version + existing title + current body
    // (for diff preview).
    let current = jc_conf::page::get(&client, id).await?;
    let current_version = current
        .version
        .as_ref()
        .map(|v| v.number)
        .ok_or_else(|| CliError::validation(format!("page {id} has no version field")))?;
    let next_version = expected_version.unwrap_or(current_version) + 1;

    let current_markdown = current
        .body
        .as_ref()
        .and_then(|b| b.as_adf())
        .as_ref()
        .map(jc_adf::to_markdown)
        .unwrap_or_default();
    let diff = unified_diff(&current_markdown, &new_markdown);

    let title = new_title.unwrap_or(&current.title);

    let req = jc_conf::page::UpdatePageRequest {
        id,
        status: "current",
        title,
        version: jc_conf::page::VersionRequest {
            number: next_version,
        },
        body: jc_conf::page::BodyRequest::from_adf(&new_adf),
    };

    let url = format!("https://{}/wiki/api/v2/pages/{}", cfg.site, id);
    let preview = Preview::new("PUT", url)
        .with_body(serde_json::to_value(&req).unwrap_or_else(|_| json!(null)))
        .with_summary(format!(
            "Update page {id} '{title}' (v{current_version} -> v{next_version})"
        ))
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

    let updated = jc_conf::page::update(&client, id, &req).await?;
    Envelope::new(json!({
        "id": updated.id,
        "title": updated.title,
        "version": updated.version.as_ref().map(|v| v.number),
    }))
    .emit();
    Ok(())
}

async fn conf_page_delete(id: &str, mode: PreviewMode) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let url = format!("https://{}/wiki/api/v2/pages/{}", cfg.site, id);
    let preview = Preview::new("DELETE", url).with_summary(format!("Delete page {id}"));

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
    jc_conf::page::delete(&client, id).await?;
    Envelope::new(json!({ "deleted": true, "id": id })).emit();
    Ok(())
}

async fn conf_space_list() -> Result<(), CliError> {
    let client = jira_client()?;
    let spaces = jc_conf::space::list(&client, &[]).await?;
    let data: Vec<_> = spaces
        .iter()
        .map(|s| {
            json!({
                "id": s.id,
                "key": s.key,
                "name": s.name,
                "type": s.space_type,
                "homepage_id": s.homepage_id,
            })
        })
        .collect();
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(spaces.len()));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn conf_space_get(key_or_id: &str) -> Result<(), CliError> {
    let client = jira_client()?;
    // If it parses as a number, treat as ID; otherwise resolve by key.
    let space = if key_or_id.chars().all(|c| c.is_ascii_digit()) {
        jc_conf::space::get(&client, key_or_id).await?
    } else {
        jc_conf::space::find_by_key(&client, key_or_id)
            .await?
            .ok_or_else(|| CliError::validation(format!("space '{key_or_id}' not found")))?
    };
    Envelope::new(json!({
        "id": space.id,
        "key": space.key,
        "name": space.name,
        "type": space.space_type,
        "homepage_id": space.homepage_id,
    }))
    .emit();
    Ok(())
}

async fn run_cql(query: &str, limit: usize, show_query: bool) -> Result<(), CliError> {
    let client = jira_client()?;
    let hits = jc_conf::search::cql(&client, query, limit).await?;

    let data: Vec<_> = hits
        .iter()
        .map(|h| {
            json!({
                "id": h.content.as_ref().map(|c| &c.id),
                "title": h.content.as_ref().map(|c| &c.title),
                "type": h.content.as_ref().map(|c| &c.content_type),
                "space_id": h.content.as_ref().and_then(|c| c.space_id.as_ref()),
                "excerpt": h.excerpt,
                "url": h.url,
                "last_modified": h.last_modified,
            })
        })
        .collect();

    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(hits.len()));
    if show_query {
        meta.insert("query".into(), json!(query));
    }
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn jira_fields_sync() -> Result<(), CliError> {
    let client = jira_client()?;
    let fields = jc_jira::fields::list_all(&client).await?;
    let cache = jc_jira::fields::FieldsCache { fields };
    let path = cache
        .save()
        .map_err(|e| CliError::io(format!("write fields cache: {e}")))?;

    Envelope::new(json!({
        "path": path.display().to_string(),
        "field_count": cache.fields.len(),
    }))
    .emit();
    Ok(())
}

fn build_fields_object(
    cache: &jc_jira::fields::FieldsCache,
    pairs: &[String],
    extras: serde_json::Map<String, Value>,
) -> Result<Value, CliError> {
    let mut obj = extras;
    for pair in pairs {
        let (raw_key, raw_val) = pair.split_once('=').ok_or_else(|| {
            CliError::validation(format!(
                "invalid --field '{pair}' (expected KEY=VALUE)"
            ))
        })?;
        let key = raw_key.trim();
        let resolved = cache
            .resolve_id(key)
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.to_string());
        let value: Value = serde_json::from_str(raw_val)
            .unwrap_or_else(|_| Value::String(raw_val.to_string()));
        obj.insert(resolved, value);
    }
    Ok(Value::Object(obj))
}

async fn jira_issue_create(
    project: &str,
    issue_type: &str,
    summary: &str,
    description_file: Option<&Path>,
    extra_fields: &[String],
    mode: PreviewMode,
) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let cache = jc_jira::fields::FieldsCache::load();

    let mut base = serde_json::Map::new();
    base.insert("project".into(), json!({ "key": project }));
    base.insert("issuetype".into(), json!({ "name": issue_type }));
    base.insert("summary".into(), json!(summary));

    if let Some(path) = description_file {
        let md = std::fs::read_to_string(path)
            .map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
        base.insert("description".into(), jc_adf::to_adf(&md));
    }

    let fields_obj = build_fields_object(&cache, extra_fields, base)?;

    let preview = Preview::new(
        "POST",
        format!("https://{}/rest/api/3/issue", cfg.site),
    )
    .with_body(json!({ "fields": fields_obj }))
    .with_summary(format!(
        "Create {issue_type} in {project}: {summary}"
    ));

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

    let created = jc_jira::issue::create(&client, &fields_obj).await?;
    Envelope::new(json!({
        "id": created.id,
        "key": created.key,
        "url": format!("https://{}/browse/{}", cfg.site, created.key),
    }))
    .emit();
    Ok(())
}

async fn jira_issue_edit(
    key: &str,
    summary: Option<&str>,
    description_file: Option<&Path>,
    extra_fields: &[String],
    mode: PreviewMode,
) -> Result<(), CliError> {
    if summary.is_none() && description_file.is_none() && extra_fields.is_empty() {
        return Err(CliError::validation(
            "at least one of --summary, --description-file, or --field is required",
        ));
    }

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let cache = jc_jira::fields::FieldsCache::load();

    let mut base = serde_json::Map::new();
    if let Some(s) = summary {
        base.insert("summary".into(), json!(s));
    }

    let mut new_description_markdown: Option<String> = None;
    if let Some(path) = description_file {
        let md = std::fs::read_to_string(path)
            .map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
        let adf = jc_adf::to_adf(&md);
        new_description_markdown = Some(jc_adf::to_markdown(&adf));
        base.insert("description".into(), adf);
    }

    let fields_obj = build_fields_object(&cache, extra_fields, base)?;

    // Fetch current issue for diff (if description is being changed).
    let diff = if new_description_markdown.is_some() {
        let current = jc_jira::issue::get(&client, key).await.ok();
        let current_md = current
            .as_ref()
            .and_then(|i| i.fields.description.as_ref())
            .map(jc_adf::to_markdown)
            .unwrap_or_default();
        Some(unified_diff(
            &current_md,
            new_description_markdown.as_deref().unwrap_or(""),
        ))
    } else {
        None
    };

    let url = format!("https://{}/rest/api/3/issue/{}", cfg.site, key);
    let mut preview = Preview::new("PUT", url)
        .with_body(json!({ "fields": fields_obj }))
        .with_summary(format!("Edit {key}"));
    if let Some(d) = diff {
        preview = preview.with_diff(d);
    }

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

    jc_jira::issue::edit(&client, key, &fields_obj).await?;
    Envelope::new(json!({
        "edited": true,
        "key": key,
    }))
    .emit();
    Ok(())
}

async fn publish(
    body_file: &Path,
    space_key: &str,
    title: &str,
    parent: Option<&str>,
    link_to: Option<&str>,
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
    let adf = jc_adf::to_adf(&md);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let space_id = jc_conf::space::resolve_id(&client, space_key).await?;

    // Step 1: Confluence create page
    let page_req = jc_conf::page::CreatePageRequest {
        space_id: &space_id,
        status: "current",
        title,
        parent_id: parent,
        body: jc_conf::page::BodyRequest::from_adf(&adf),
    };
    let page_preview = Preview::new("POST", format!("https://{}/wiki/api/v2/pages", cfg.site))
        .with_body(serde_json::to_value(&page_req).unwrap_or_else(|_| json!(null)))
        .with_summary(format!("Create page '{title}' in space {space_key}"));

    // Step 2: Jira comment (optional) — templated since real page URL is
    // only known after create succeeds.
    let link_preview = link_to.map(|issue_key| {
        let template_url = format!(
            "https://{}/wiki/spaces/{}/pages/<NEW_PAGE_ID>",
            cfg.site, space_key
        );
        let template_md = format!("Published to Confluence: [{title}]({template_url})\n");
        let comment_adf = jc_adf::to_adf(&template_md);
        Preview::new(
            "POST",
            format!("https://{}/rest/api/3/issue/{}/comment", cfg.site, issue_key),
        )
        .with_body(json!({ "body": comment_adf }))
        .with_summary(format!("Link published page in Jira issue {issue_key}"))
    });

    match mode {
        PreviewMode::DryRun => {
            let mut previews = vec![page_preview];
            if let Some(lp) = link_preview {
                previews.push(lp);
            }
            emit_composite_dry_run(&previews);
            return Ok(());
        }
        PreviewMode::Confirm => {
            let total = 1 + link_preview.as_ref().map_or(0, |_| 1);
            eprintln!("--- publish preview ---");
            eprintln!("\n# Step 1 of {total}: Confluence page creation");
            page_preview.render_to_stderr()?;
            if let Some(lp) = &link_preview {
                eprintln!("\n# Step 2 of {total}: Jira linking comment");
                lp.render_to_stderr()?;
                eprintln!(
                    "\nNote: <NEW_PAGE_ID> in the comment body will be substituted \
                     with the real page ID after step 1 succeeds."
                );
            }
            if !prompt_yes_no("Send all? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    // Execute step 1
    let page = jc_conf::page::create(&client, &page_req).await?;
    let page_url = format!(
        "https://{}/wiki/spaces/{}/pages/{}",
        cfg.site, space_key, page.id
    );

    let mut data = json!({
        "page": {
            "id": page.id,
            "title": page.title,
            "space_id": page.space_id,
            "parent_id": page.parent_id,
            "version": page.version.as_ref().map(|v| v.number),
            "url": page_url,
        },
        "comment": Value::Null,
    });

    let mut warnings: Vec<String> = Vec::new();

    // Execute step 2 if requested. Partial failure surfaces as a warning
    // rather than a hard error — the page is already created and linking
    // it can be retried manually.
    if let Some(issue_key) = link_to {
        let real_md = format!("Published to Confluence: [{title}]({page_url})\n");
        let comment_adf = jc_adf::to_adf(&real_md);
        match jc_jira::comment::add(&client, issue_key, &comment_adf).await {
            Ok(c) => {
                data["comment"] = json!({
                    "id": c.id,
                    "issue": issue_key,
                    "created": c.created,
                });
            }
            Err(e) => {
                warnings.push(format!(
                    "page {} created successfully but linking comment on {issue_key} failed: {e}",
                    page.id
                ));
            }
        }
    }

    let mut env = Envelope::new(data);
    env.warnings = warnings;
    env.emit();
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
