use std::path::PathBuf;

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

    /// Raw JQL query. Auto-paginates; use --limit to cap results.
    Jql {
        /// The JQL query to execute.
        query: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum JiraIssueCommand {
    /// Fetch an issue by key, with description rendered as markdown.
    Get {
        /// Issue key, e.g. FOO-123
        key: String,
    },

    /// List issues matching the given filters. Filters AND together.
    List {
        /// Project key (e.g. FOO)
        #[arg(long)]
        project: Option<String>,
        /// Status name (e.g. "In Progress")
        #[arg(long)]
        status: Option<String>,
        /// Assignee: account ID, display name, or `me` (= currentUser())
        #[arg(long)]
        assignee: Option<String>,
        /// Issue type name (e.g. Bug, Story)
        #[arg(long = "type")]
        issue_type: Option<String>,
        /// Updated within JQL time expression (e.g. -7d, -1w, -24h)
        #[arg(long)]
        updated: Option<String>,
    },

    /// List issues assigned to the current user.
    Mine {
        /// Optional status filter (e.g. "In Progress")
        #[arg(long)]
        status: Option<String>,
    },

    /// Fuzzy-search issues by summary text.
    Search {
        /// Search terms (matched against issue summary)
        terms: String,
        /// Restrict to a single project
        #[arg(long)]
        project: Option<String>,
    },

    /// Comment operations.
    #[command(subcommand)]
    Comment(JiraCommentCommand),

    /// Attachment operations.
    #[command(subcommand)]
    Attachment(JiraAttachmentCommand),

    /// Transition an issue to another workflow state.
    Transition {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Target status name (fuzzy-matched against available transitions)
        #[arg(long)]
        to: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum JiraAttachmentCommand {
    /// List attachments on an issue.
    List {
        /// Issue key, e.g. FOO-123
        key: String,
    },
    /// Download an attachment to disk. Writes to `<out-dir>/<filename>`
    /// and prints the path so Claude Code can read the file.
    Get {
        /// Attachment ID (from `attachment list` or `issue get`)
        id: String,
        /// Output directory for downloaded file
        #[arg(long = "out-dir", default_value = "./attachments")]
        out_dir: PathBuf,
    },
    /// Upload a file as an attachment on an issue.
    Upload {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Path to the file to upload
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum JiraCommentCommand {
    /// Add a comment to an issue. Body is read from a markdown file and
    /// converted to ADF before sending.
    Add {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Path to a markdown file containing the comment body
        #[arg(long = "body-file")]
        body_file: PathBuf,
    },

    /// List comments on an issue, bodies rendered as markdown.
    List {
        /// Issue key, e.g. FOO-123
        key: String,
    },

    /// Replace a comment's body. Shows a diff against the current remote
    /// state in the preview.
    Edit {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Comment ID to edit
        comment_id: String,
        /// Path to a markdown file containing the new body
        #[arg(long = "body-file")]
        body_file: PathBuf,
    },

    /// Delete a comment.
    Delete {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Comment ID to delete
        comment_id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfCommand {
    // Populated as endpoints land. `page get` is the first planned command.
}
