use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "jc",
    version,
    about = "Jira + Confluence CLI designed for Claude Code consumption",
    long_about = "jc is a single-binary CLI for Jira Cloud and Confluence Cloud. \
                  All output is JSON on stdout. All errors are JSON on stderr. \
                  Every mutating command supports --dry-run."
)]
pub struct Cli {
    /// Log HTTP request/response pairs to stderr (auth redacted).
    #[arg(long, global = true)]
    pub verbose: bool,

    /// Print the outgoing HTTP request as JSON, do not send it. Exit 0.
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// Preview the request and block for y/N on stdin. Interactive only.
    #[arg(long, global = true, conflicts_with = "dry_run")]
    pub confirm: bool,

    /// Cap results for list/search commands. 0 = unlimited (default).
    #[arg(long, global = true, default_value_t = 0)]
    pub limit: usize,

    /// For wrapper commands, print the compiled JQL/CQL alongside results.
    #[arg(long, global = true)]
    pub show_query: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Configuration and auth management.
    #[command(subcommand)]
    Config(ConfigCommand),

    /// Jira Cloud operations.
    #[command(subcommand)]
    Jira(JiraCommand),

    /// Confluence Cloud operations.
    #[command(subcommand)]
    Conf(ConfCommand),
    // publish lands once page create + jira comment add are implemented.
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print resolved config, token redacted.
    Show,
    /// Verify auth end-to-end by calling /rest/api/3/myself.
    Test,
}

#[derive(Debug, Subcommand)]
pub enum JiraCommand {
    /// Issue-level operations.
    #[command(subcommand)]
    Issue(JiraIssueCommand),
}

#[derive(Debug, Subcommand)]
pub enum JiraIssueCommand {
    /// Fetch an issue by key, with description rendered as markdown.
    Get {
        /// Issue key, e.g. FOO-123
        key: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfCommand {
    // Populated as endpoints land. `page get` is the first planned command.
}
