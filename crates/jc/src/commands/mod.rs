use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use jc_core::Client;
use jc_core::literal;
use jc_jira::jql::JqlBuilder;
use jc_jira::transitions::{self, MatchResult};
use serde_json::{Value, json};
use similar::TextDiff;

use crate::cli::{
    Cli, Command, ConfAttachmentCommand, ConfCommand, ConfPageCommand, ConfSpaceCommand,
    ConfigCommand, FieldsSubcommand, JiraAttachmentCommand, JiraCommand, JiraCommentCommand,
    JiraIssueCommand, JiraLinkCommand, JiraUserCommand,
};
use crate::config::Config;
use crate::markdown_images::{FoundImage, find_local_images, rewrite_image_urls};
use crate::markdown_mentions::{
    ResolvedMention, apply_mentions_to_adf, find_mention_queries, resolve_mentions,
    rewrite_mentions,
};
use crate::output::{CliError, Envelope};
use crate::preview::{Preview, PreviewMode, emit_composite_dry_run, prompt_yes_no};

pub async fn dispatch(args: Cli) -> Result<(), CliError> {
    let limit = args.limit;
    let show_query = args.show_query;
    let mode = PreviewMode::from_flags(args.dry_run, args.confirm);

    match args.command {
        Command::Config(ConfigCommand::Show) => config_show(),
        Command::Config(ConfigCommand::Test) => config_test().await,
        Command::Config(ConfigCommand::Set { key, value }) => config_set(&key, &value),

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
                if !literal::is_valid_relative_time(&u) {
                    return Err(CliError::validation(format!(
                        "invalid --updated value '{u}'. Expected a JQL relative \
                         time expression like -7d, -24h, or 2w. For complex \
                         time filters use `jc jira jql` directly."
                    )));
                }
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
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Comment(JiraCommentCommand::Add {
            key,
            body_file,
        }))) => jira_comment_add(&key, &body_file, mode).await,
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

        Command::Jira(JiraCommand::User(JiraUserCommand::Me)) => jira_user_me().await,
        Command::Jira(JiraCommand::User(JiraUserCommand::Search { query })) => {
            jira_user_search(&query, limit).await
        }

        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Assign { key, to })) => {
            jira_issue_assign(&key, &to, mode).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Watch { key })) => {
            jira_issue_watch(&key, mode).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Unwatch { key })) => {
            jira_issue_unwatch(&key, mode).await
        }
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Link(link_cmd))) => {
            jira_issue_link(link_cmd, mode).await
        }

        Command::Conf(ConfCommand::Page(ConfPageCommand::Get { id })) => conf_page_get(&id).await,
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
        })) => conf_page_create(&space, &title, &from_markdown, parent.as_deref(), mode).await,
        Command::Conf(ConfCommand::Page(ConfPageCommand::Update {
            id,
            from_markdown,
            title,
            expected_version,
        })) => {
            conf_page_update(
                &id,
                &from_markdown,
                title.as_deref(),
                expected_version,
                mode,
            )
            .await
        }
        Command::Conf(ConfCommand::Page(ConfPageCommand::Delete { id })) => {
            conf_page_delete(&id, mode).await
        }
        Command::Conf(ConfCommand::Space(ConfSpaceCommand::List)) => conf_space_list().await,
        Command::Conf(ConfCommand::Space(ConfSpaceCommand::Get { key_or_id })) => {
            conf_space_get(&key_or_id).await
        }
        Command::Conf(ConfCommand::Cql { query }) => run_cql(&query, limit, show_query).await,
        Command::Conf(ConfCommand::Attachment(ConfAttachmentCommand::List { page })) => {
            conf_attachment_list(&page, limit).await
        }
        Command::Conf(ConfCommand::Attachment(ConfAttachmentCommand::Get { id, out_dir })) => {
            conf_attachment_get(&id, &out_dir).await
        }
        Command::Conf(ConfCommand::Attachment(ConfAttachmentCommand::Upload { page, file })) => {
            conf_attachment_upload(&page, &file, mode).await
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

fn config_set(key: &str, value: &str) -> Result<(), CliError> {
    let allowed = ["site", "email", "token"];
    if !allowed.contains(&key) {
        return Err(CliError::validation(format!(
            "unknown config key '{key}' (expected one of: {})",
            allowed.join(", ")
        )));
    }

    // A literal "-" means "read the value from stdin". This keeps secrets
    // out of argv — which gets captured by shell history, ps output, and
    // sometimes logged by the OS — while still supporting the friendly
    // `jc config set token <value>` form for quick setup.
    let actual_value: String = if value == "-" {
        use std::io::Read as _;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| CliError::io(format!("read stdin: {e}")))?;
        let trimmed = buf.trim_end_matches(['\r', '\n']).to_string();
        if trimmed.is_empty() {
            return Err(CliError::validation("stdin was empty"));
        }
        trimmed
    } else {
        value.to_string()
    };

    crate::config::write_keychain(key, &actual_value)?;
    Envelope::new(json!({
        "key": key,
        "stored": true,
        "source": "keychain",
    }))
    .emit();
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

    let description_markdown = issue.fields.description.as_ref().map(jc_adf::to_markdown);

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

async fn jira_comment_add(key: &str, body_file: &Path, mode: PreviewMode) -> Result<(), CliError> {
    let md = std::fs::read_to_string(body_file)
        .map_err(|e| CliError::io(format!("read {}: {e}", body_file.display())))?;
    if md.trim().is_empty() {
        return Err(CliError::validation(format!(
            "body file {} is empty",
            body_file.display()
        )));
    }

    let base_dir = base_dir_of(body_file);
    let images = find_local_images(&md, base_dir);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let mentions = resolve_mentions_for(&client, &md).await?;

    let url = format!("https://{}/rest/api/3/issue/{}/comment", cfg.site, key);

    // Preview is built from the ORIGINAL markdown (local image paths
    // still intact) with mentions already resolved. Upload+rewrite
    // happens after the confirmation gate so a cancelled --confirm
    // doesn't leave orphaned attachments.
    let preview_adf = compile_adf(&md, &mentions);
    let preview = Preview::new("POST", url)
        .with_body(json!({ "body": preview_adf }))
        .with_summary(format!("Add comment to {key}"));

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    let (replacements, uploaded) = upload_images_for_jira_issue(&client, key, &images).await?;
    let final_md = rewrite_image_urls(&md, &replacements);
    let final_adf = compile_adf(&final_md, &mentions);

    let comment = jc_jira::comment::add(&client, key, &final_adf).await?;

    let mut env = Envelope::new(json!({
        "id": comment.id,
        "created": comment.created,
        "author": comment.author.as_ref().map(|u| &u.display_name),
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    if !uploaded.is_empty() {
        env.warnings
            .push(format!("uploaded {} image(s) to {key}", uploaded.len()));
    }
    if !mentions.is_empty() {
        env.warnings
            .push(format!("resolved {} mention(s)", mentions.len()));
    }
    env.emit();
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

    let base_dir = base_dir_of(body_file);
    let images = find_local_images(&md, base_dir);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let mentions = resolve_mentions_for(&client, &md).await?;

    // Convert the ORIGINAL md (with mentions resolved) for preview;
    // real upload+rewrite happens after the confirmation gate.
    let preview_adf = compile_adf(&md, &mentions);
    let preview_markdown = jc_adf::to_markdown(&preview_adf);

    // Fetch current state for the diff preview.
    let current = jc_jira::comment::get(&client, key, comment_id).await?;
    let current_markdown = current
        .body
        .as_ref()
        .map(jc_adf::to_markdown)
        .unwrap_or_default();

    let diff = unified_diff(&current_markdown, &preview_markdown);

    let url = format!(
        "https://{}/rest/api/3/issue/{}/comment/{}",
        cfg.site, key, comment_id
    );
    let preview = Preview::new("PUT", url)
        .with_body(json!({ "body": preview_adf }))
        .with_summary(format!("Edit comment {comment_id} on {key}"))
        .with_diff(diff);

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    let (replacements, uploaded) = upload_images_for_jira_issue(&client, key, &images).await?;
    let final_md = rewrite_image_urls(&md, &replacements);
    let final_adf = compile_adf(&final_md, &mentions);

    let updated = jc_jira::comment::edit(&client, key, comment_id, &final_adf).await?;
    let mut env = Envelope::new(json!({
        "id": updated.id,
        "updated": updated.updated,
        "author": updated.author.as_ref().map(|u| &u.display_name),
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    if !uploaded.is_empty() {
        env.warnings
            .push(format!("uploaded {} image(s) to {key}", uploaded.len()));
    }
    if !mentions.is_empty() {
        env.warnings
            .push(format!("resolved {} mention(s)", mentions.len()));
    }
    env.emit();
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
    let preview =
        Preview::new("DELETE", url).with_summary(format!("Delete comment {comment_id} on {key}"));

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

async fn jira_issue_transition(key: &str, target: &str, mode: PreviewMode) -> Result<(), CliError> {
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

    let path = safe_write(out_dir, &meta.filename, &blob.bytes)?;

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

/// Write `bytes` under `out_dir` using a server-supplied `filename`,
/// defending against path traversal and symlink-follow attacks.
///
/// 1. Extract only the final path component (`Path::file_name`) — rejects
///    `../` traversal even if the server supplied a multi-segment path.
/// 2. Reject reserved names (`.`, `..`, empty).
/// 3. Refuse to overwrite an existing file that is a symlink, so a
///    pre-planted symlink at the target can't be used to write outside
///    `out_dir` or clobber arbitrary files.
fn safe_write(out_dir: &Path, filename: &str, bytes: &[u8]) -> Result<PathBuf, CliError> {
    // Strip the path to just the file component. This defangs
    // "../../etc/passwd", "foo/bar.txt", "C:\\windows\\...", etc.
    let raw_name = Path::new(filename)
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| {
            CliError::validation(format!("server returned unsafe filename: {filename:?}"))
        })?;

    if raw_name.is_empty() || raw_name == "." || raw_name == ".." {
        return Err(CliError::validation(format!(
            "server returned reserved filename: {raw_name:?}"
        )));
    }

    std::fs::create_dir_all(out_dir)
        .map_err(|e| CliError::io(format!("mkdir {}: {e}", out_dir.display())))?;

    let path = out_dir.join(raw_name);

    // If the target already exists as a symlink, refuse — an attacker
    // could have pre-planted one pointing at an arbitrary file.
    if let Ok(meta) = std::fs::symlink_metadata(&path)
        && meta.file_type().is_symlink()
    {
        return Err(CliError::validation(format!(
            "refusing to write through existing symlink: {}",
            path.display()
        )));
    }

    // Use OpenOptions with truncate/create rather than fs::write so the
    // behavior is explicit and auditable.
    use std::io::Write as _;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&path)
        .map_err(|e| CliError::io(format!("open {}: {e}", path.display())))?;
    file.write_all(bytes)
        .map_err(|e| CliError::io(format!("write {}: {e}", path.display())))?;

    Ok(path)
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

async fn jira_attachment_upload(key: &str, file: &Path, mode: PreviewMode) -> Result<(), CliError> {
    let bytes =
        std::fs::read(file).map_err(|e| CliError::io(format!("read {}: {e}", file.display())))?;
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
    let uploaded = jc_jira::attachments::upload(&client, key, &filename, bytes, mime).await?;

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

/// Upload a single local image file to a Jira issue and return the
/// resulting attachment id. Used by the image pre-processor for
/// `--body-file` / `--description-file` handlers whose target is an
/// existing issue.
async fn upload_image_to_jira_issue(
    client: &Client,
    issue_key: &str,
    path: &Path,
) -> Result<String, CliError> {
    let bytes =
        std::fs::read(path).map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::validation(format!("invalid filename: {}", path.display())))?
        .to_string();
    let mime = guess_mime_from_ext(path);
    let uploaded = jc_jira::attachments::upload(client, issue_key, &filename, bytes, mime).await?;
    uploaded.into_iter().next().map(|a| a.id).ok_or_else(|| {
        CliError::validation(format!(
            "upload to {issue_key} returned no attachment metadata"
        ))
    })
}

/// Same idea for Confluence pages.
async fn upload_image_to_conf_page(
    client: &Client,
    page_id: &str,
    path: &Path,
) -> Result<String, CliError> {
    let bytes =
        std::fs::read(path).map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
    let filename = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::validation(format!("invalid filename: {}", path.display())))?
        .to_string();
    let mime = guess_mime_from_ext(path);
    let uploaded = jc_conf::attachments::upload(client, page_id, &filename, bytes, mime).await?;
    uploaded.into_iter().next().map(|a| a.id).ok_or_else(|| {
        CliError::validation(format!(
            "upload to page {page_id} returned no attachment metadata"
        ))
    })
}

/// Upload every image in `images` to the given Jira issue and return
/// the replacement list for [`rewrite_image_urls`] plus a JSON record
/// of what was uploaded for the command result envelope.
async fn upload_images_for_jira_issue(
    client: &Client,
    issue_key: &str,
    images: &[FoundImage],
) -> Result<(Vec<(String, String)>, Vec<Value>), CliError> {
    let mut replacements = Vec::with_capacity(images.len());
    let mut record = Vec::with_capacity(images.len());
    for img in images {
        let id = upload_image_to_jira_issue(client, issue_key, &img.resolved_path).await?;
        replacements.push((img.original_url.clone(), format!("attachment:{id}")));
        record.push(json!({
            "original_url": img.original_url,
            "path": img.resolved_path.display().to_string(),
            "attachment_id": id,
        }));
    }
    Ok((replacements, record))
}

async fn upload_images_for_conf_page(
    client: &Client,
    page_id: &str,
    images: &[FoundImage],
) -> Result<(Vec<(String, String)>, Vec<Value>), CliError> {
    let mut replacements = Vec::with_capacity(images.len());
    let mut record = Vec::with_capacity(images.len());
    for img in images {
        let id = upload_image_to_conf_page(client, page_id, &img.resolved_path).await?;
        replacements.push((img.original_url.clone(), format!("attachment:{id}")));
        record.push(json!({
            "original_url": img.original_url,
            "path": img.resolved_path.display().to_string(),
            "attachment_id": id,
        }));
    }
    Ok((replacements, record))
}

/// Convenience: the parent directory of a markdown source file, used
/// as the base for resolving relative image URLs. Falls back to `.`
/// when the file has no parent (e.g. a bare filename in CWD).
fn base_dir_of(file: &Path) -> &Path {
    file.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

/// Resolve every `@[query]` token in `md` via the user search API.
/// Returns an empty map when the source has no mentions so the caller
/// can skip the API round-trip entirely for the common case.
async fn resolve_mentions_for(
    client: &Client,
    md: &str,
) -> Result<BTreeMap<String, ResolvedMention>, CliError> {
    let queries = find_mention_queries(md);
    if queries.is_empty() {
        Ok(BTreeMap::new())
    } else {
        resolve_mentions(client, &queries).await
    }
}

/// Compile a markdown body into ADF with mentions resolved.
///
/// Order of operations:
/// 1. `rewrite_mentions` normalizes each `@[query]` token to
///    `@[accountId]` using the resolution map.
/// 2. `jc_adf::to_adf` runs the standard markdown → ADF conversion.
/// 3. `apply_mentions_to_adf` walks the ADF tree and splits
///    `@[accountId]` tokens inside unmarked text nodes into proper
///    `mention` inline nodes.
///
/// This is called once to build the preview (before images are
/// uploaded) and again to build the final payload (after image URLs
/// have been rewritten to `attachment:` form).
fn compile_adf(md: &str, mentions: &BTreeMap<String, ResolvedMention>) -> Value {
    let rewritten = rewrite_mentions(md, mentions);
    let mut adf = jc_adf::to_adf(&rewritten);
    apply_mentions_to_adf(&mut adf, mentions);
    adf
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
    // CQL shares JQL's string literal grammar: `literal::escape_string`
    // correctly handles both backslashes and double quotes (the previous
    // inline replace handled only `"`, leaving a backslash-injection hole).
    let mut query = format!(
        "type = \"page\" AND text ~ {}",
        literal::escape_string(terms)
    );
    if let Some(s) = space {
        query.push_str(&format!(" AND space = {}", literal::escape_string(s)));
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

    let base_dir = base_dir_of(body_file);
    let images = find_local_images(&md, base_dir);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let mentions = resolve_mentions_for(&client, &md).await?;
    let space_id = jc_conf::space::resolve_id(&client, space_key).await?;

    // Phase 1 ADF: convert original md (local image paths still intact
    // as link marks) with mentions already resolved. Uploads happen
    // after create.
    let initial_adf = compile_adf(&md, &mentions);

    let req = jc_conf::page::CreatePageRequest {
        space_id: &space_id,
        status: "current",
        title,
        parent_id: parent,
        body: jc_conf::page::BodyRequest::from_adf(&initial_adf),
    };

    let url = format!("https://{}/wiki/api/v2/pages", cfg.site);
    let preview = Preview::new("POST", url)
        .with_body(serde_json::to_value(&req).unwrap_or_else(|_| json!(null)))
        .with_summary(format!("Create page '{title}' in space {space_key}"));

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    // Phase 1: create page with initial ADF.
    let page = jc_conf::page::create(&client, &req).await?;

    // Phase 2: upload images to the new page; follow-up update with
    // rewritten body that references the real attachment IDs.
    let (replacements, uploaded) = upload_images_for_conf_page(&client, &page.id, &images).await?;

    let mut warnings: Vec<String> = Vec::new();
    let final_version = if !replacements.is_empty() {
        let rewritten = rewrite_image_urls(&md, &replacements);
        let fixed_adf = compile_adf(&rewritten, &mentions);
        let current_version = page.version.as_ref().map(|v| v.number).unwrap_or(1);
        let next_version = current_version + 1;
        let update_req = jc_conf::page::UpdatePageRequest {
            id: &page.id,
            status: "current",
            title,
            version: jc_conf::page::VersionRequest {
                number: next_version,
            },
            body: jc_conf::page::BodyRequest::from_adf(&fixed_adf),
        };
        match jc_conf::page::update(&client, &page.id, &update_req).await {
            Ok(updated) => {
                warnings.push(format!(
                    "uploaded {} image(s) and updated page body",
                    uploaded.len()
                ));
                updated.version.as_ref().map(|v| v.number)
            }
            Err(e) => {
                warnings.push(format!(
                    "page {} created but image-aware follow-up update failed: {e}",
                    page.id
                ));
                page.version.as_ref().map(|v| v.number)
            }
        }
    } else {
        page.version.as_ref().map(|v| v.number)
    };
    if !mentions.is_empty() {
        warnings.push(format!("resolved {} mention(s)", mentions.len()));
    }

    let mut env = Envelope::new(json!({
        "id": page.id,
        "title": page.title,
        "space_id": page.space_id,
        "parent_id": page.parent_id,
        "version": final_version,
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    env.warnings = warnings;
    env.emit();
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

    let base_dir = base_dir_of(body_file);
    let images = find_local_images(&md, base_dir);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let mentions = resolve_mentions_for(&client, &md).await?;

    // Convert original md (with mentions resolved) for the preview.
    // Upload+rewrite happens after confirmation.
    let preview_adf = compile_adf(&md, &mentions);
    let preview_markdown = jc_adf::to_markdown(&preview_adf);

    // Fetch current page to obtain version + existing title + current body.
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
    let diff = unified_diff(&current_markdown, &preview_markdown);

    let title = new_title.unwrap_or(&current.title);

    let preview_req = jc_conf::page::UpdatePageRequest {
        id,
        status: "current",
        title,
        version: jc_conf::page::VersionRequest {
            number: next_version,
        },
        body: jc_conf::page::BodyRequest::from_adf(&preview_adf),
    };

    let url = format!("https://{}/wiki/api/v2/pages/{}", cfg.site, id);
    let preview = Preview::new("PUT", url)
        .with_body(serde_json::to_value(&preview_req).unwrap_or_else(|_| json!(null)))
        .with_summary(format!(
            "Update page {id} '{title}' (v{current_version} -> v{next_version})"
        ))
        .with_diff(diff);

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    // Upload images to the existing page, rewrite markdown, rebuild request.
    let (replacements, uploaded) = upload_images_for_conf_page(&client, id, &images).await?;
    let final_md = rewrite_image_urls(&md, &replacements);
    let final_adf = compile_adf(&final_md, &mentions);

    let final_req = jc_conf::page::UpdatePageRequest {
        id,
        status: "current",
        title,
        version: jc_conf::page::VersionRequest {
            number: next_version,
        },
        body: jc_conf::page::BodyRequest::from_adf(&final_adf),
    };

    let updated = jc_conf::page::update(&client, id, &final_req).await?;
    let mut env = Envelope::new(json!({
        "id": updated.id,
        "title": updated.title,
        "version": updated.version.as_ref().map(|v| v.number),
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    if !uploaded.is_empty() {
        env.warnings
            .push(format!("uploaded {} image(s) to page {id}", uploaded.len()));
    }
    if !mentions.is_empty() {
        env.warnings
            .push(format!("resolved {} mention(s)", mentions.len()));
    }
    env.emit();
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

async fn jira_user_me() -> Result<(), CliError> {
    let client = jira_client()?;
    let me = jc_jira::users::myself(&client).await?;
    Envelope::new(json!({
        "account_id": me.account_id,
        "display_name": me.display_name,
        "email": me.email_address,
        "active": me.active,
    }))
    .emit();
    Ok(())
}

async fn jira_user_search(query: &str, limit: usize) -> Result<(), CliError> {
    let client = jira_client()?;
    let max = if limit == 0 { 50 } else { limit };
    let users = jc_jira::users::search(&client, query, max).await?;
    let data: Vec<_> = users
        .iter()
        .map(|u| {
            json!({
                "account_id": u.account_id,
                "display_name": u.display_name,
                "email": u.email_address,
            })
        })
        .collect();
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(users.len()));
    meta.insert("query".into(), json!(query));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

/// Resolve an assignee descriptor ("me", accountId, or free text) to an
/// accountId. "none" disassigns.
async fn resolve_assignee(client: &Client, who: &str) -> Result<Option<String>, CliError> {
    let trimmed = who.trim();
    if trimmed.eq_ignore_ascii_case("none") || trimmed.eq_ignore_ascii_case("null") {
        return Ok(None);
    }
    if trimmed.eq_ignore_ascii_case("me") || trimmed.eq_ignore_ascii_case("currentuser") {
        let me = jc_jira::users::myself(client).await?;
        return Ok(Some(me.account_id));
    }
    // Heuristic: looks like an accountId (alphanum + dashes, no spaces, long)
    if !trimmed.contains(' ') && !trimmed.contains('@') && trimmed.len() >= 20 {
        return Ok(Some(trimmed.to_string()));
    }
    // Otherwise search.
    let users = jc_jira::users::search(client, trimmed, 5).await?;
    let first = users
        .into_iter()
        .next()
        .ok_or_else(|| CliError::validation(format!("no user matches '{who}'")))?;
    Ok(Some(first.account_id))
}

async fn jira_issue_assign(key: &str, to: &str, mode: PreviewMode) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let account_id = resolve_assignee(&client, to).await?;

    let url = format!("https://{}/rest/api/3/issue/{}/assignee", cfg.site, key);
    let body_json = json!({ "accountId": account_id });
    let preview = Preview::new("PUT", url)
        .with_body(body_json)
        .with_summary(match &account_id {
            Some(id) => format!("Assign {key} to {id}"),
            None => format!("Unassign {key}"),
        });

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

    jc_jira::issue::assign(&client, key, account_id.as_deref()).await?;
    Envelope::new(json!({
        "assigned": account_id.is_some(),
        "key": key,
        "account_id": account_id,
    }))
    .emit();
    Ok(())
}

async fn jira_issue_watch(key: &str, mode: PreviewMode) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let me = jc_jira::users::myself(&client).await?;

    let url = format!("https://{}/rest/api/3/issue/{}/watchers", cfg.site, key);
    let preview = Preview::new("POST", url)
        .with_body(json!(me.account_id))
        .with_summary(format!("Watch {key} as {}", me.display_name));

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

    jc_jira::issue::add_watcher(&client, key, &me.account_id).await?;
    Envelope::new(json!({ "watched": true, "key": key })).emit();
    Ok(())
}

async fn jira_issue_unwatch(key: &str, mode: PreviewMode) -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let me = jc_jira::users::myself(&client).await?;

    let url = format!(
        "https://{}/rest/api/3/issue/{}/watchers?accountId={}",
        cfg.site, key, me.account_id
    );
    let preview =
        Preview::new("DELETE", url).with_summary(format!("Unwatch {key} as {}", me.display_name));

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

    jc_jira::issue::remove_watcher(&client, key, &me.account_id).await?;
    Envelope::new(json!({ "unwatched": true, "key": key })).emit();
    Ok(())
}

async fn jira_issue_link(cmd: JiraLinkCommand, mode: PreviewMode) -> Result<(), CliError> {
    match cmd {
        JiraLinkCommand::List { key } => {
            let client = jira_client()?;
            let links = jc_jira::issue_links::list_on_issue(&client, &key).await?;
            let data: Vec<_> = links
                .iter()
                .map(|l| {
                    json!({
                        "id": l.id,
                        "type": l.link_type.name,
                        "outward": l.link_type.outward,
                        "inward": l.link_type.inward,
                        "inward_issue": l.inward_issue.as_ref().map(|i| json!({
                            "key": i.key,
                            "summary": i.fields.as_ref().and_then(|f| f.summary.as_ref()),
                            "status": i.fields.as_ref().and_then(|f| f.status.as_ref().map(|s| &s.name)),
                        })),
                        "outward_issue": l.outward_issue.as_ref().map(|i| json!({
                            "key": i.key,
                            "summary": i.fields.as_ref().and_then(|f| f.summary.as_ref()),
                            "status": i.fields.as_ref().and_then(|f| f.status.as_ref().map(|s| &s.name)),
                        })),
                    })
                })
                .collect();
            let mut env = Envelope::new(data);
            let mut meta = serde_json::Map::new();
            meta.insert("count".into(), json!(links.len()));
            meta.insert("issue".into(), json!(key));
            env.meta = Some(Value::Object(meta));
            env.emit();
            Ok(())
        }
        JiraLinkCommand::Add { key, to, link_type } => {
            let cfg = Config::from_env()?;
            let url = format!("https://{}/rest/api/3/issueLink", cfg.site);
            let body = json!({
                "type": { "name": link_type },
                "inwardIssue": { "key": key },
                "outwardIssue": { "key": to },
            });
            let preview = Preview::new("POST", url)
                .with_body(body)
                .with_summary(format!("Link {key} -[{link_type}]-> {to}"));

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
            jc_jira::issue_links::add(&client, &link_type, &key, &to).await?;
            Envelope::new(json!({
                "linked": true,
                "from": key,
                "to": to,
                "type": link_type,
            }))
            .emit();
            Ok(())
        }
        JiraLinkCommand::Remove { link_id } => {
            let cfg = Config::from_env()?;
            let url = format!("https://{}/rest/api/3/issueLink/{}", cfg.site, link_id);
            let preview =
                Preview::new("DELETE", url).with_summary(format!("Remove issue link {link_id}"));

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
            jc_jira::issue_links::remove(&client, &link_id).await?;
            Envelope::new(json!({ "removed": true, "id": link_id })).emit();
            Ok(())
        }
        JiraLinkCommand::Types => {
            let client = jira_client()?;
            let types = jc_jira::issue_links::list_types(&client).await?;
            let data: Vec<_> = types
                .iter()
                .map(|t| {
                    json!({
                        "id": t.id,
                        "name": t.name,
                        "inward": t.inward,
                        "outward": t.outward,
                    })
                })
                .collect();
            let mut env = Envelope::new(data);
            let mut meta = serde_json::Map::new();
            meta.insert("count".into(), json!(types.len()));
            env.meta = Some(Value::Object(meta));
            env.emit();
            Ok(())
        }
    }
}

async fn conf_attachment_list(page_id: &str, limit: usize) -> Result<(), CliError> {
    let client = jira_client()?;
    let atts = jc_conf::attachments::list_on_page(&client, page_id, limit).await?;
    let data: Vec<_> = atts
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "title": a.title,
                "media_type": a.media_type,
                "file_size": a.file_size,
                "page_id": a.page_id,
                "download_link": a.download_link,
            })
        })
        .collect();
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(atts.len()));
    meta.insert("page".into(), json!(page_id));
    env.meta = Some(Value::Object(meta));
    env.emit();
    Ok(())
}

async fn conf_attachment_get(id: &str, out_dir: &Path) -> Result<(), CliError> {
    let client = jira_client()?;
    let (meta_rec, blob) = jc_conf::attachments::download(&client, id).await?;

    let path = safe_write(out_dir, &meta_rec.title, &blob.bytes)?;

    let mime = blob.content_type.clone().or(meta_rec.media_type.clone());
    let warning = unreadable_mime_warning(mime.as_deref());

    let mut env = Envelope::new(json!({
        "id": meta_rec.id,
        "title": meta_rec.title,
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

async fn conf_attachment_upload(
    page_id: &str,
    file: &Path,
    mode: PreviewMode,
) -> Result<(), CliError> {
    let bytes =
        std::fs::read(file).map_err(|e| CliError::io(format!("read {}: {e}", file.display())))?;
    let filename = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| CliError::validation(format!("invalid filename: {}", file.display())))?
        .to_string();
    let mime = guess_mime_from_ext(file);

    let cfg = Config::from_env()?;
    let url = format!(
        "https://{}/wiki/rest/api/content/{}/child/attachment",
        cfg.site, page_id
    );
    let preview = Preview::new("POST", url).with_summary(format!(
        "Upload {filename} ({} bytes) to page {page_id}",
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
    let uploaded = jc_conf::attachments::upload(&client, page_id, &filename, bytes, mime).await?;
    let data: Vec<_> = uploaded
        .iter()
        .map(|a| {
            json!({
                "id": a.id,
                "title": a.title,
                "type": a.content_type,
            })
        })
        .collect();
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("count".into(), json!(uploaded.len()));
    meta.insert("page".into(), json!(page_id));
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
            CliError::validation(format!("invalid --field '{pair}' (expected KEY=VALUE)"))
        })?;
        let key = raw_key.trim();
        let resolved = cache
            .resolve_id(key)
            .map(|s| s.to_string())
            .unwrap_or_else(|| key.to_string());
        let value: Value =
            serde_json::from_str(raw_val).unwrap_or_else(|_| Value::String(raw_val.to_string()));
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

    // Read markdown up front; image uploads happen AFTER create since
    // the target issue doesn't exist yet.
    let (md_source, images) = if let Some(path) = description_file {
        let s = std::fs::read_to_string(path)
            .map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
        let imgs = find_local_images(&s, base_dir_of(path));
        (Some(s), imgs)
    } else {
        (None, Vec::new())
    };

    // Resolve @[query] mentions before conversion so the preview body
    // contains real mention nodes.
    let mentions = match md_source.as_deref() {
        Some(md) => resolve_mentions_for(&client, md).await?,
        None => BTreeMap::new(),
    };

    let mut base = serde_json::Map::new();
    base.insert("project".into(), json!({ "key": project }));
    base.insert("issuetype".into(), json!({ "name": issue_type }));
    base.insert("summary".into(), json!(summary));
    if let Some(md) = md_source.as_deref() {
        base.insert("description".into(), compile_adf(md, &mentions));
    }

    let fields_obj = build_fields_object(&cache, extra_fields, base)?;

    let preview = Preview::new("POST", format!("https://{}/rest/api/3/issue", cfg.site))
        .with_body(json!({ "fields": fields_obj }))
        .with_summary(format!("Create {issue_type} in {project}: {summary}"));

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    // Phase 1: create the issue. Its description (if any) still
    // references local image paths as links — we'll fix those up
    // in phase 2.
    let created = jc_jira::issue::create(&client, &fields_obj).await?;

    // Phase 2: upload images to the new issue (if any) and do a
    // follow-up edit to replace local paths with real `attachment:ID`
    // references in the description.
    let (replacements, uploaded) =
        upload_images_for_jira_issue(&client, &created.key, &images).await?;

    let mut warnings: Vec<String> = Vec::new();
    if !replacements.is_empty()
        && let Some(md) = md_source.as_deref()
    {
        let rewritten = rewrite_image_urls(md, &replacements);
        let fixed_adf = compile_adf(&rewritten, &mentions);
        let edit_fields = json!({ "description": fixed_adf });
        if let Err(e) = jc_jira::issue::edit(&client, &created.key, &edit_fields).await {
            warnings.push(format!(
                "issue {} created but description follow-up edit failed: {e}",
                created.key
            ));
        } else {
            warnings.push(format!(
                "uploaded {} image(s) and updated description on {}",
                uploaded.len(),
                created.key
            ));
        }
    }
    if !mentions.is_empty() {
        warnings.push(format!("resolved {} mention(s)", mentions.len()));
    }

    let mut env = Envelope::new(json!({
        "id": created.id,
        "key": created.key,
        "url": format!("https://{}/browse/{}", cfg.site, created.key),
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    env.warnings = warnings;
    env.emit();
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

    // Read markdown and find images up front; the upload happens after
    // the preview/confirm gate below.
    let (md_source, images, base_dir_buf) = if let Some(path) = description_file {
        let s = std::fs::read_to_string(path)
            .map_err(|e| CliError::io(format!("read {}: {e}", path.display())))?;
        let base = base_dir_of(path).to_path_buf();
        let imgs = find_local_images(&s, &base);
        (Some(s), imgs, Some(base))
    } else {
        (None, Vec::new(), None)
    };
    let _ = base_dir_buf; // retained for clarity; the paths inside `images` are already resolved

    // Resolve any @[query] mentions in the description up front so the
    // preview shows the proper mention nodes.
    let mentions = match md_source.as_deref() {
        Some(md) => resolve_mentions_for(&client, md).await?,
        None => BTreeMap::new(),
    };

    // Build the preview fields with the ORIGINAL markdown so local
    // image paths show up in the preview. The real upload+rewrite
    // happens after confirmation.
    let mut preview_base = serde_json::Map::new();
    if let Some(s) = summary {
        preview_base.insert("summary".into(), json!(s));
    }
    let preview_new_markdown = md_source.as_ref().map(|md| {
        let adf = compile_adf(md, &mentions);
        let rendered = jc_adf::to_markdown(&adf);
        preview_base.insert("description".into(), adf);
        rendered
    });
    let preview_fields = build_fields_object(&cache, extra_fields, preview_base)?;

    // Fetch current issue for diff (if description is being changed).
    let diff = if preview_new_markdown.is_some() {
        let current = jc_jira::issue::get(&client, key).await.ok();
        let current_md = current
            .as_ref()
            .and_then(|i| i.fields.description.as_ref())
            .map(jc_adf::to_markdown)
            .unwrap_or_default();
        Some(unified_diff(
            &current_md,
            preview_new_markdown.as_deref().unwrap_or(""),
        ))
    } else {
        None
    };

    let url = format!("https://{}/rest/api/3/issue/{}", cfg.site, key);
    let mut preview = Preview::new("PUT", url)
        .with_body(json!({ "fields": preview_fields }))
        .with_summary(format!("Edit {key}"));
    if let Some(d) = diff {
        preview = preview.with_diff(d);
    }

    match mode {
        PreviewMode::DryRun => {
            emit_dry_run_with_image_note(&preview, &images);
            return Ok(());
        }
        PreviewMode::Confirm => {
            render_preview_with_images(&preview, &images)?;
            if !prompt_yes_no("Send? [y/N]: ")? {
                emit_cancelled();
                return Ok(());
            }
        }
        PreviewMode::Send => {}
    }

    // Upload images (if any), then rebuild the fields object with the
    // rewritten markdown so the description references real attachments.
    let (replacements, uploaded) = upload_images_for_jira_issue(&client, key, &images).await?;

    let mut final_base = serde_json::Map::new();
    if let Some(s) = summary {
        final_base.insert("summary".into(), json!(s));
    }
    if let Some(md) = md_source.as_deref() {
        let rewritten = rewrite_image_urls(md, &replacements);
        final_base.insert("description".into(), compile_adf(&rewritten, &mentions));
    }
    let final_fields = build_fields_object(&cache, extra_fields, final_base)?;

    jc_jira::issue::edit(&client, key, &final_fields).await?;

    let mut env = Envelope::new(json!({
        "edited": true,
        "key": key,
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    }));
    if !uploaded.is_empty() {
        env.warnings
            .push(format!("uploaded {} image(s) to {key}", uploaded.len()));
    }
    if !mentions.is_empty() {
        env.warnings
            .push(format!("resolved {} mention(s)", mentions.len()));
    }
    env.emit();
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

    let base_dir = base_dir_of(body_file);
    let images = find_local_images(&md, base_dir);

    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let mentions = resolve_mentions_for(&client, &md).await?;

    // Phase 1 ADF: mentions resolved, images still as links. Phase 2
    // rewrites images after the new page is created.
    let adf = compile_adf(&md, &mentions);

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
            format!(
                "https://{}/rest/api/3/issue/{}/comment",
                cfg.site, issue_key
            ),
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

    // Step 1b: upload local images to the new page and follow up with an
    // update that rewrites the body to reference real attachment IDs.
    let (replacements, uploaded) = upload_images_for_conf_page(&client, &page.id, &images).await?;
    let mut warnings: Vec<String> = Vec::new();
    let final_version = if !replacements.is_empty() {
        let rewritten = rewrite_image_urls(&md, &replacements);
        let fixed_adf = compile_adf(&rewritten, &mentions);
        let current_version = page.version.as_ref().map(|v| v.number).unwrap_or(1);
        let next_version = current_version + 1;
        let update_req = jc_conf::page::UpdatePageRequest {
            id: &page.id,
            status: "current",
            title,
            version: jc_conf::page::VersionRequest {
                number: next_version,
            },
            body: jc_conf::page::BodyRequest::from_adf(&fixed_adf),
        };
        match jc_conf::page::update(&client, &page.id, &update_req).await {
            Ok(updated) => {
                warnings.push(format!(
                    "uploaded {} image(s) and updated page body",
                    uploaded.len()
                ));
                updated.version.as_ref().map(|v| v.number)
            }
            Err(e) => {
                warnings.push(format!(
                    "page {} created but image-aware follow-up update failed: {e}",
                    page.id
                ));
                page.version.as_ref().map(|v| v.number)
            }
        }
    } else {
        page.version.as_ref().map(|v| v.number)
    };
    if !mentions.is_empty() {
        warnings.push(format!("resolved {} mention(s)", mentions.len()));
    }

    let mut data = json!({
        "page": {
            "id": page.id,
            "title": page.title,
            "space_id": page.space_id,
            "parent_id": page.parent_id,
            "version": final_version,
            "url": page_url,
        },
        "comment": Value::Null,
        "uploaded_images": uploaded,
        "resolved_mentions": mentions_summary(&mentions),
    });

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

/// Emit a dry-run envelope for a mutation that has local-image uploads
/// pending. The preview itself reflects the request body as it would be
/// sent WITHOUT the uploads (local paths still in place); a warning
/// lists the images that would actually be uploaded when the command
/// runs for real.
fn emit_dry_run_with_image_note(preview: &Preview, images: &[FoundImage]) {
    let data = json!({ "preview": preview, "will_send": false });
    let mut env = Envelope::new(data);
    let mut meta = serde_json::Map::new();
    meta.insert("mode".into(), json!("dry_run"));
    env.meta = Some(Value::Object(meta));
    for img in images {
        env.warnings.push(format!(
            "would upload {} (resolved: {})",
            img.original_url,
            img.resolved_path.display()
        ));
    }
    env.emit();
}

/// Render a preview to stderr for confirm mode, followed by the list of
/// images that will be uploaded after the user confirms. The user sees
/// both the converted request body AND the pending upload list before
/// typing y/N.
fn render_preview_with_images(preview: &Preview, images: &[FoundImage]) -> Result<(), CliError> {
    eprintln!("--- preview ---");
    preview.render_to_stderr()?;
    if !images.is_empty() {
        eprintln!("\n--- images to upload on confirm ---");
        for img in images {
            eprintln!("  {} -> {}", img.original_url, img.resolved_path.display());
        }
    }
    Ok(())
}

/// Build the `resolved_mentions` block for a result envelope so the
/// user / Claude Code can see which queries were resolved to which
/// accountIds.
fn mentions_summary(mentions: &BTreeMap<String, ResolvedMention>) -> Value {
    let list: Vec<Value> = mentions
        .iter()
        .map(|(query, m)| {
            json!({
                "query": query,
                "account_id": m.account_id,
                "display_name": m.display_name,
            })
        })
        .collect();
    Value::Array(list)
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
