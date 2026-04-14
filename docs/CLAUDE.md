# jc — reference for Claude Code

`jc` is a single binary that talks to Jira Cloud and Confluence Cloud on
behalf of the authenticated user. You are the primary consumer. Output is
JSON on stdout, errors are JSON on stderr, and every mutation supports
`--dry-run`.

## First check

`jc --help` and `jc <subcommand> --help` are always authoritative. This doc
shows patterns; `--help` shows current syntax.

## Auth

Three env vars: `JC_SITE`, `JC_EMAIL`, `JC_TOKEN`.
Verify with `jc config test` before doing anything else in a fresh session.

## Output shape

**Success** (stdout):

```json
{
  "data": { ... },
  "warnings": ["..."],
  "meta": { "count": 42, "query": "...", "took_ms": 380 }
}
```

`warnings` may contain things like "this issue has 3 non-inline attachments,
use `jc jira issue attachment list` to see them" — don't ignore it.

**Error** (stderr):

```json
{
  "error": {
    "status": 403,
    "code": "FORBIDDEN",
    "messages": ["You do not have permission..."],
    "field_errors": {},
    "request_id": "...",
    "endpoint": "POST /rest/api/3/issue"
  }
}
```

Exit codes: `0` success, `1` usage error, `2` API error, `3` auth/config
error, `4` validation error (dry-run rejected the input).

## Common workflows

### Pull a ticket and its context

```sh
jc jira issue get FOO-123
```

Returns the issue with `description` as markdown (converted from ADF),
`comments.count`, and `attachments.count`.

### Fuzzy-find a ticket by title

```sh
jc jira issue search "payment webhook retry"
```

Compiles to `summary ~ "payment webhook retry"` under the hood. Returns all
matches, no pagination cap by default. Pass `--show-query` to see the JQL.

### Read linked Confluence docs

```sh
jc conf page search "payment webhook"
jc conf page get <ID>
```

Body is returned as markdown, ready to reason over.

### Download an attachment

```sh
jc jira issue attachment list FOO-123
jc jira issue attachment get <ATTACHMENT_ID>
```

Output is `{"path": "./attachments/FOO-123/design.pdf", "mime": "...", "warning": null}`.
Use separate parsing tools (pdftotext, image readers) on the returned path.
If the mime type is unusual, `warning` will be set — surface it to the user.

### Post a comment (markdown input)

```sh
cat > /tmp/note.md <<'EOF'
# Investigation

Root cause: the retry count is off-by-one in `webhook_handler.rs:142`.
EOF

# 1. Preview first
jc jira issue comment add FOO-123 --body-file /tmp/note.md --dry-run

# 2. Show the preview to the user, get confirmation

# 3. Send for real
jc jira issue comment add FOO-123 --body-file /tmp/note.md
```

The converter handles GFM tables, code blocks with language hints, links,
headings, lists, and `@user` mentions. Exotic ADF nodes are preserved as
fenced blocks with a type marker (` ```adf:panel:info`) — don't strip them.

### Transition a ticket

```sh
jc jira issue transition FOO-123 --to "In Review"
```

Fuzzy-matches on the available transitions for that issue's workflow.

### Publish a markdown doc to Confluence and link it in a ticket

```sh
jc publish ./design.md \
  --space ENG --title "Payment retry design" \
  --parent 12345 --link-to FOO-123 \
  --dry-run
```

Composite command: previews the Confluence page payload AND the Jira comment
that will follow. On real run, creates the page, then posts a comment on the
Jira ticket linking to it. Atomic from the user's view — if the comment post
fails, the page creation is reported as a partial success in `warnings`.

### Raw JQL / CQL (escape hatch)

```sh
jc jira jql 'project = FOO AND updated >= -7d ORDER BY updated DESC'
jc conf cql 'space = ENG AND text ~ "webhook"'
```

Use when the wrapper commands aren't expressive enough.

## Dry-run discipline

Every mutating command supports `--dry-run`. The pattern:

1. Build the command with `--dry-run`.
2. Show the JSON preview to the user.
3. If they confirm, re-run without the flag.

For `edit` operations, the preview includes a unified diff against the
current remote state, not just the outgoing payload. This is how "append my
own considerations before pushing" becomes reviewable.

## Custom fields

Jira custom fields (`customfield_10042`) are presented by human name ("Story
Points", "Epic Link"). The name↔ID map is cached at `~/.cache/jc/fields.json`.
If a field is missing, run `jc jira fields sync` to refresh.

## Pagination

Auto-paginate by default. `--limit 0` = all results (default). Pass a
positive integer to cap. You should almost never need to deal with cursor
state yourself — the client loops internally.

## Rate limits

Atlassian uses dynamic cost-based rate limiting. On 429 the client honors
`Retry-After` and retries automatically. 5xx also retries with backoff. 4xx
errors other than 429 do not retry — they surface immediately as structured
errors.

## What `jc` intentionally does NOT do

- GitLab / GitHub integration (use `gh` / `glab`)
- Local mirroring, spreadsheets, database dumps
- Multi-site / profile switching (deferred)
- MCP server mode
- Any undocumented Atlassian API

If you need one of these, that is a separate tool's job.
