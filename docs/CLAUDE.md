# jc — reference for Claude Code

`jc` is a single binary that talks to Jira Cloud and Confluence Cloud on
behalf of the authenticated user. You are the primary consumer. Output is
JSON on stdout, errors are JSON on stderr, and every mutation supports
`--dry-run`.

## First check

`jc --help` and `jc <subcommand> --help` are always authoritative. This
doc shows patterns; `--help` shows current syntax.

## Auth

Config is read from env vars first, then the OS keychain. Required keys:
`site`, `email`, `token`.

- Env vars: `JC_SITE`, `JC_EMAIL`, `JC_TOKEN`
- Keychain: service `jc`, accounts `site` / `email` / `token`
- Store a value: `jc config set <key> <value>` (writes to keychain)
- Verify: `jc config test` (calls `/rest/api/3/myself`)
- Inspect: `jc config show` (token redacted)

## Global flags

- `--dry-run` — print the exact HTTP request that would be sent as JSON
  on stdout, exit 0, do not send anything
- `--confirm` — render the preview to stderr, block on stdin `y/N`,
  send if confirmed
- `--verbose` — log each HTTP request/response (method, URL, status) to
  stderr, authorization always redacted
- `--limit N` — cap list/search results. `0` = unlimited (default)
- `--show-query` — echo compiled JQL/CQL in `meta.query` for wrapper
  commands

## Output shape

Success (stdout):

```json
{
  "data": { ... },
  "warnings": ["..."],
  "meta": { "count": 42, "query": "...", "mode": "dry_run" }
}
```

Error (stderr):

```json
{
  "error": {
    "status": 403,
    "code": "FORBIDDEN",
    "messages": ["..."],
    "field_errors": { ... }
  }
}
```

Exit codes: `0` success · `1` usage/io · `2` API error · `3` auth/config
error · `4` validation error.

## Dry-run discipline

For any mutation, run with `--dry-run` first, show the JSON preview to
the user, re-run without the flag once approved. The preview struct:

```json
{
  "method": "POST",
  "url": "https://.../rest/api/3/issue/FOO-123/comment",
  "headers": { "Authorization": "Basic ***", ... },
  "body": { ... },
  "summary": "Add comment to FOO-123",
  "diff": "--- before\n+++ after\n..."
}
```

Edit commands populate `diff` with a unified diff against current remote
state (markdown-level for ADF fields) so the user sees exactly what is
changing.

Composite commands (`jc publish`) emit `{"previews": [...]}` with
`meta.step_count` so each planned step is visible.

## Common workflows

### Pull a ticket and its context

```sh
jc jira issue get FOO-123
```

Returns the issue with `description_markdown` (converted from ADF),
`comments.count`, `attachments[]`, labels, etc.

### Fuzzy-find a ticket

```sh
jc jira issue search "payment webhook retry"
# or: jc jira issue list --project FOO --status "In Progress" --assignee me
# or: jc jira issue mine --status "In Progress"
# or: jc jira jql 'project = FOO AND updated >= -7d ORDER BY updated DESC'
```

All four are wrappers over / escape hatches to the new
`POST /rest/api/3/search/jql` cursor-paginated endpoint.

### Read linked Confluence docs

```sh
jc conf page search "payment webhook"       # wraps CQL
jc conf page get <PAGE_ID>                  # body rendered as markdown
# Or full CQL: jc conf cql 'space = ENG AND text ~ "webhook"'
```

### Download an attachment

```sh
jc jira issue attachment list FOO-123
jc jira issue attachment get <ATTACHMENT_ID> [--out-dir ./attachments]
# Confluence side:
jc conf attachment list --page <PAGE_ID>
jc conf attachment get <ATTACHMENT_ID>
```

Output: `{"path": "...", "size": ..., "mime": "..."}`. A `warnings[]`
entry is added when the mime type is unlikely to be directly readable
(use a dedicated parser for those).

### Post a markdown comment on a Jira ticket

```sh
jc jira issue comment add FOO-123 --body-file note.md --dry-run
jc jira issue comment add FOO-123 --body-file note.md
```

Also available: `comment list <KEY>`, `comment edit <KEY> <ID>
--body-file ...` (shows diff), `comment delete <KEY> <ID>`.

### Transition a ticket

```sh
jc jira issue transition FOO-123 --to "In Review"
```

Fuzzy-matches against available transitions for the issue's workflow.
Ambiguous / not-found cases error out with the candidate list.

### Create or edit an issue

```sh
jc jira issue create --project FOO --type Bug --summary "..." \
  --description-file desc.md \
  --field "Story Points=5" --field "Epic Link=FOO-100"

jc jira issue edit FOO-123 --summary "..." --description-file new.md
```

`--field KEY=VALUE` accepts human field names ("Story Points"); the
local cache at `~/.cache/jc/fields.json` translates to `customfield_*`
IDs. Refresh with `jc jira fields sync`. `VALUE` is parsed as JSON when
possible (numbers, arrays), falling back to string.

### Assign / watch / link

```sh
jc jira issue assign FOO-123 --to me
jc jira issue assign FOO-123 --to "Alice Smith"
jc jira issue assign FOO-123 --to none              # unassign

jc jira issue watch FOO-123
jc jira issue unwatch FOO-123

jc jira issue link list FOO-123
jc jira issue link add FOO-123 --to FOO-456 --type Blocks
jc jira issue link remove <LINK_ID>
jc jira issue link types
```

Assignee resolution: `me` → `currentUser()`; long alphanumeric strings
are treated as accountIds; anything else is fed to user search.

### User lookups

```sh
jc jira user me
jc jira user search "alice@example.com"
```

### Publish a markdown file as a Confluence page and link it on a Jira ticket

```sh
jc publish ./design.md \
  --space ENG --title "Payment retry design" \
  --parent 12345 --link-to FOO-123 --dry-run
```

Composite command — two steps, one preview:

1. Create Confluence page in the given space
2. Post a linking comment on the Jira issue

Dry-run emits a `{"previews": [...]}` array. Confirm mode renders both
steps with headers and one combined y/N prompt. On live send, partial
failure (page created, comment failed) surfaces the failure as a
`warnings[]` entry rather than a hard error — the page exists and the
linking step can be retried.

### Confluence pages

```sh
jc conf page get <ID>
jc conf page list --space ENG [--parent <PARENT_ID>]
jc conf page search "..." [--space ENG]
jc conf page create --space ENG --title "..." --from-markdown doc.md [--parent <ID>]
jc conf page update <ID> --from-markdown doc.md [--title "..." --expected-version N]
jc conf page delete <ID>

jc conf space list
jc conf space get <KEY_OR_ID>
```

`conf page update` fetches current state, computes a markdown-level
diff, and uses the current version + 1 (or `--expected-version` if
supplied to avoid version races).

### Raw escape hatches

```sh
jc jira jql 'project = FOO AND updated >= -7d ORDER BY updated DESC'
jc conf cql 'space = ENG AND text ~ "webhook"'
```

Both auto-paginate (cursor for JQL, start/limit for CQL).

## Custom fields

`jc jira fields sync` refreshes `~/.cache/jc/fields.json` from
`/rest/api/3/field`. The cache is pure — never authoritative, rebuild
any time. `--field KEY=VALUE` accepts either human names or raw
`customfield_*` IDs.

## Rate limits

Atlassian uses dynamic cost-based rate limiting with 429 + `Retry-After`.
The HTTP layer returns structured errors on non-2xx; callers surface
them as JSON on stderr. Manual retry for now; automated backoff is a
planned follow-up.

## What `jc` intentionally does NOT do

- GitLab / GitHub integration — use `gh` / `glab`
- Local mirroring, spreadsheets, or database dumps
- Multi-site / profile switching (deferred)
- MCP server mode
- Any undocumented Atlassian API
