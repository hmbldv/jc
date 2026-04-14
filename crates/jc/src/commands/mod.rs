use crate::cli::{Cli, Command, ConfigCommand, JiraCommand, JiraIssueCommand};
use crate::config::Config;
use crate::output::{CliError, Envelope};

pub async fn dispatch(args: Cli) -> Result<(), CliError> {
    match args.command {
        Command::Config(ConfigCommand::Show) => config_show(),
        Command::Config(ConfigCommand::Test) => config_test().await,
        Command::Jira(JiraCommand::Issue(JiraIssueCommand::Get { key })) => {
            jira_issue_get(&key).await
        }
        Command::Conf(_) => {
            unreachable!("conf subcommands not yet implemented")
        }
    }
}

fn config_show() -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    Envelope::new(cfg.redacted_json()).emit();
    Ok(())
}

async fn config_test() -> Result<(), CliError> {
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let me = jc_jira::users::myself(&client).await?;
    Envelope::new(serde_json::json!({
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
    let cfg = Config::from_env()?;
    let client = cfg.jira_client()?;
    let issue = jc_jira::issue::get(&client, key).await?;

    let description_markdown = issue
        .fields
        .description
        .as_ref()
        .map(jc_adf::to_markdown);

    let data = serde_json::json!({
        "id": issue.id,
        "key": issue.key,
        "summary": issue.fields.summary,
        "issue_type": issue.fields.issuetype.as_ref().map(|t| &t.name),
        "status": issue.fields.status.as_ref().map(|s| &s.name),
        "status_category": issue.fields.status.as_ref().and_then(|s| s.category.as_ref().map(|c| &c.key)),
        "priority": issue.fields.priority.as_ref().map(|p| &p.name),
        "assignee": issue.fields.assignee.as_ref().map(|u| serde_json::json!({
            "account_id": u.account_id,
            "display_name": u.display_name,
        })),
        "reporter": issue.fields.reporter.as_ref().map(|u| serde_json::json!({
            "account_id": u.account_id,
            "display_name": u.display_name,
        })),
        "labels": issue.fields.labels,
        "description_markdown": description_markdown,
        "comments": {
            "count": issue.fields.comment.as_ref().map(|c| c.total).unwrap_or(0),
        },
        "attachments": issue.fields.attachment.iter().map(|a| serde_json::json!({
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
