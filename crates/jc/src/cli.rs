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

    /// Publish a markdown file as a Confluence page, optionally linking
    /// it from a Jira issue in the same step.
    Publish {
        /// Path to the markdown file to publish
        file: PathBuf,
        /// Space key (e.g. ENG)
        #[arg(long)]
        space: String,
        /// Page title
        #[arg(long)]
        title: String,
        /// Optional parent page ID
        #[arg(long)]
        parent: Option<String>,
        /// Optional Jira issue key to post a linking comment on
        #[arg(long = "link-to")]
        link_to: Option<String>,
    },
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

    /// Refresh the local custom-field cache.
    Fields(FieldsCommand),

    /// User operations.
    #[command(subcommand)]
    User(JiraUserCommand),
}

#[derive(Debug, Subcommand)]
pub enum JiraUserCommand {
    /// Print the current authenticated user.
    Me,
    /// Search users by email, display name, or accountId.
    Search {
        /// Search query
        query: String,
    },
}

#[derive(Debug, clap::Args)]
pub struct FieldsCommand {
    #[command(subcommand)]
    pub command: FieldsSubcommand,
}

#[derive(Debug, Subcommand)]
pub enum FieldsSubcommand {
    /// Fetch the full field catalog and write it to ~/.cache/jc/fields.json
    Sync,
}

#[derive(Debug, Subcommand)]
pub enum JiraIssueCommand {
    /// Fetch an issue by key, with description rendered as markdown.
    Get {
        /// Issue key, e.g. FOO-123
        key: String,
    },

    /// Create a new issue.
    Create {
        /// Project key (e.g. FOO)
        #[arg(long)]
        project: String,
        /// Issue type name (e.g. Bug, Story)
        #[arg(long = "type")]
        issue_type: String,
        /// Summary (title)
        #[arg(long)]
        summary: String,
        /// Optional path to markdown file for the description
        #[arg(long = "description-file")]
        description_file: Option<PathBuf>,
        /// Additional fields as KEY=VALUE (human name or customfield_ID).
        /// VALUE is parsed as JSON if possible, otherwise treated as string.
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
    },

    /// Edit an existing issue.
    Edit {
        /// Issue key, e.g. FOO-123
        key: String,
        /// New summary
        #[arg(long)]
        summary: Option<String>,
        /// Path to markdown file for the new description
        #[arg(long = "description-file")]
        description_file: Option<PathBuf>,
        /// Additional fields as KEY=VALUE
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
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

    /// Assign (or unassign) an issue.
    Assign {
        /// Issue key, e.g. FOO-123
        key: String,
        /// Assignee: "me", accountId, email, or display name. Use "none" to unassign.
        #[arg(long)]
        to: String,
    },

    /// Add the current user as a watcher on an issue.
    Watch {
        /// Issue key, e.g. FOO-123
        key: String,
    },

    /// Remove the current user as a watcher on an issue.
    Unwatch {
        /// Issue key, e.g. FOO-123
        key: String,
    },

    /// Issue link operations.
    #[command(subcommand)]
    Link(JiraLinkCommand),
}

#[derive(Debug, Subcommand)]
pub enum JiraLinkCommand {
    /// List links on an issue.
    List {
        /// Issue key, e.g. FOO-123
        key: String,
    },
    /// Add a link. "<KEY> --type Blocks --to OTHER" means "KEY blocks OTHER".
    Add {
        /// Source issue key
        key: String,
        /// Target issue key
        #[arg(long)]
        to: String,
        /// Link type name (e.g. Blocks, Relates, Duplicate)
        #[arg(long = "type")]
        link_type: String,
    },
    /// Remove a link by its ID (from `link list`).
    Remove {
        /// Link ID
        link_id: String,
    },
    /// List all available link types in the site.
    Types,
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
    /// Page operations.
    #[command(subcommand)]
    Page(ConfPageCommand),

    /// Space operations.
    #[command(subcommand)]
    Space(ConfSpaceCommand),

    /// Attachment operations.
    #[command(subcommand)]
    Attachment(ConfAttachmentCommand),

    /// Raw CQL query.
    Cql {
        /// The CQL query to execute.
        query: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfAttachmentCommand {
    /// List attachments on a page.
    List {
        /// Page ID
        #[arg(long)]
        page: String,
    },
    /// Download an attachment to disk.
    Get {
        /// Attachment ID
        id: String,
        /// Output directory for downloaded file
        #[arg(long = "out-dir", default_value = "./attachments")]
        out_dir: PathBuf,
    },
    /// Upload a file as an attachment on a page.
    Upload {
        /// Target page ID
        #[arg(long)]
        page: String,
        /// Path to the file to upload
        file: PathBuf,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfPageCommand {
    /// Fetch a page by ID, body rendered as markdown.
    Get {
        /// Page ID
        id: String,
    },
    /// List pages in a space, or children of a specific parent page.
    List {
        /// Space key (e.g. ENG)
        #[arg(long)]
        space: String,
        /// Restrict to direct children of this parent page ID
        #[arg(long)]
        parent: Option<String>,
    },
    /// Search pages by CQL text match.
    Search {
        /// Search terms (matched against title and content)
        terms: String,
        /// Restrict to a single space (by key)
        #[arg(long)]
        space: Option<String>,
    },
    /// Create a new page from a markdown file.
    Create {
        /// Space key (e.g. ENG)
        #[arg(long)]
        space: String,
        /// Page title
        #[arg(long)]
        title: String,
        /// Path to markdown file containing the page body
        #[arg(long = "from-markdown")]
        from_markdown: PathBuf,
        /// Optional parent page ID
        #[arg(long)]
        parent: Option<String>,
    },
    /// Replace a page's body. Shows a diff against current state in preview.
    Update {
        /// Page ID
        id: String,
        /// Path to markdown file containing the new body
        #[arg(long = "from-markdown")]
        from_markdown: PathBuf,
        /// Override the title (keeps existing if omitted)
        #[arg(long)]
        title: Option<String>,
        /// Override the expected current version number (default: fetch)
        #[arg(long = "expected-version")]
        expected_version: Option<u64>,
    },
    /// Delete a page.
    Delete {
        /// Page ID
        id: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum ConfSpaceCommand {
    /// List all spaces.
    List,
    /// Look up a space by key or numeric ID.
    Get {
        /// Space key (e.g. ENG) or numeric ID
        key_or_id: String,
    },
}
