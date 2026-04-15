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
- Keychain: service `dev.hmbldv.jc`, accounts `site` / `email` / `token`
- Store a value: `jc config set <key> <value>` (writes to keychain).
  Pass `-` as the value to read from stdin — that keeps the token out
  of shell history, `ps` output, and argv logs.
- Verify: `jc config test` (calls `/rest/api/3/myself`)
- Inspect: `jc config show` (token redacted; reports which sources it
  used so you can see whether env or keychain provided each value)

## Global flags

- `--dry-run` — print the exact HTTP request that would be sent as JSON
  on stdout, exit 0, do not send anything
- `--confirm` — render the preview to stderr, block on stdin `y/N`,
  send if confirmed. Errors immediately if stdin is not a TTY.
- `--verbose` — log each HTTP request/response (method, URL, status) to
  stderr. Authorization is always redacted; query strings are replaced
  with `?<redacted>` so signed-URL redirect targets don't leak.
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

Write commands that took a markdown body also include
`uploaded_images: [...]` and `resolved_mentions: [...]` in the data
payload so you can surface what was uploaded or linked.

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

Commands whose markdown body references local images populate the
dry-run envelope's `warnings[]` with one entry per `would upload`
target. The preview body itself still shows the ORIGINAL markdown
(local paths intact); uploads only happen on real send.

Composite commands (`jc publish`) emit `{"previews": [...]}` with
`meta.step_count` so each planned step is visible.

## Markdown-native rich content

Any command that takes a `--body-file`, `--description-file`, or
`--from-markdown` path supports the full rich-content set. Use these
syntaxes freely — `jc` handles the plumbing.

### Local image embedding

```markdown
Here is the design:

![architecture](./diagram.png)

And a screenshot of the failure: ![bug](../screenshots/err.jpg)
```

Rules:
- URLs with a scheme (`http://`, `https://`, `attachment:`, `data:`)
  are left alone.
- Relative paths are resolved against the markdown file's parent
  directory, not the CWD.
- Each unique local URL uploads once even if referenced multiple times.
- The uploaded attachment id comes back in `data.uploaded_images[]`.
- On **edit** commands (target exists): upload happens between the
  confirmation gate and the send, so cancelling `--confirm` leaves no
  orphans.
- On **create** commands (target doesn't exist yet — `issue create`,
  `page create`, `publish`): uploads run AFTER the target is created,
  followed by a second-phase edit/update that rewrites the body to
  reference real attachment IDs. Partial failure of the follow-up
  surfaces as a `warnings[]` entry rather than a hard error.

### @mention resolution

```markdown
cc @[alice@example.com] and @[Alice Smith] for review
```

Query forms accepted inside `@[...]`:
- Full accountId (long alphanumeric, skips the user-search round-trip)
- Email address (resolved by exact match)
- Display name or substring (exact case-insensitive match wins over
  partial matches; a single partial match is accepted; multiple partial
  matches error with the candidate list so you can disambiguate)

Resolved mentions are returned in `data.resolved_mentions[]` with the
original query, the resolved accountId, and the display name.
Mentions inside marked text (bold, italic, code) are intentionally
left as plain text because ADF mention nodes don't support marks —
splitting a marked run would silently drop formatting.

### Exotic ADF nodes (escape hatch)

```markdown
```adf:panel
{"type":"panel","attrs":{"panelType":"info"},"content":[
  {"type":"paragraph","content":[{"type":"text","text":"heads up"}]}
]}
```
```

Any ADF node type you can't express in standard markdown can be written
as a fenced code block with info string `adf:<type>` and a body of raw
JSON. The converter re-inflates the node verbatim on both directions,
and fence length auto-scales so nested backticks inside the JSON
can't break out.

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

Server-supplied filenames are path-traversal-safe: only the final
component is kept, `.` / `..` / empty names are rejected, and the
writer refuses to overwrite a pre-existing symlink at the target.

### Post a markdown comment on a Jira ticket

```sh
cat > /tmp/note.md <<'EOF'
# Investigation

Root cause: the retry count is off-by-one.

cc @[alice@example.com]

![repro](./repro.png)
EOF

jc jira issue comment add FOO-123 --body-file /tmp/note.md --dry-run
jc jira issue comment add FOO-123 --body-file /tmp/note.md
```

The dry-run shows the planned preview; the real send uploads the
image, resolves the mention, and attaches the result to the comment.

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

If the markdown contains local images, they're uploaded to the new
page after step 1 and the body is updated with the real attachment
refs as part of the same invocation.

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

## Rate limits and retries

Atlassian uses dynamic cost-based rate limiting. `jc` handles 429s
automatically: up to 4 attempts per request with `Retry-After` honored
(capped at 120s — beyond that the CLI gives up and surfaces the error
so you can rerun later). Reads (GET / HEAD / download) also retry on
502/503/504; mutations (POST/PUT/DELETE) only retry on 429 to avoid
double-committing when a 5xx indicates partial processing. Multipart
uploads are single-attempt because the request body can't be rebuilt.

## What `jc` intentionally does NOT do

- GitLab / GitHub integration — use `gh` / `glab`
- Local mirroring, spreadsheets, or database dumps
- Multi-site / profile switching (deferred)
- MCP server mode
- Any undocumented Atlassian API
