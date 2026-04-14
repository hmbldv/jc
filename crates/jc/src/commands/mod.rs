use crate::cli::{Cli, Command, ConfigCommand};
use crate::config::Config;
use crate::output::{CliError, Envelope};

pub async fn dispatch(args: Cli) -> Result<(), CliError> {
    match args.command {
        Command::Config(ConfigCommand::Show) => {
            let cfg = Config::from_env()?;
            Envelope::new(cfg.redacted_json()).emit();
            Ok(())
        }
        Command::Config(ConfigCommand::Test) => {
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
        Command::Jira(_) | Command::Conf(_) => {
            // Subcommand trees are empty at scaffold time — clap rejects
            // unknown variants before reaching here.
            unreachable!("jira/conf subcommands not yet implemented")
        }
    }
}
