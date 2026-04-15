# jc

**J**ira + **C**onfluence CLI, designed for Claude Code to consume.

A single Rust binary that turns Atlassian Cloud into a programmatic surface:
JSON-first output, markdown-native input, `--dry-run` on every mutation, and
full CRUD parity with whatever the authenticated user can do in the UI.

## Why this exists

Existing Jira / Confluence CLIs and MCP servers have been too limited, too
opinionated, or too fragile for real agent workflows. `jc` is opinionated the
other direction: it treats Claude Code as the primary consumer and humans as
secondary, collapses the Jira and Confluence APIs into one coherent tool, and
stays out of the way on formatting.

See [`docs/OVERVIEW.md`](docs/OVERVIEW.md) for the full scope and rationale.
See [`docs/CLAUDE.md`](docs/CLAUDE.md) for the pattern-oriented reference
that Claude Code reads when using the tool.

## Install

```sh
cargo install --path crates/jc
```

Or build from source:

```sh
cargo build --release
./target/release/jc --help
```

Update an already-installed copy (from a fresh pull):

```sh
cd ~/Repositories/hmbldv/jc/main && git pull && \
  cargo install --path crates/jc --force
```

Or install directly from GitHub without a local clone:

```sh
cargo install --git https://github.com/hmbldv/jc --force
```

## Quickstart

Create an API token at <https://id.atlassian.com/manage-profile/security/api-tokens>,
then either export env vars:

```sh
export JC_SITE=your-org.atlassian.net
export JC_EMAIL=you@example.com
export JC_TOKEN=...
```

Or store them in the OS keychain (service `dev.hmbldv.jc`):

```sh
jc config set site  your-org.atlassian.net
jc config set email you@example.com
jc config set token -           # reads from stdin so the token stays out of shell history
```

Env vars take precedence when both are set. Verify:

```sh
jc config test   # calls /rest/api/3/myself, exits 0 on success
jc config show   # redacted; reports which sources were used
```

## Feature overview

**Jira**
- `jc jira issue {get, create, edit, list, mine, search, transition,
  assign, watch, unwatch}`
- `jc jira issue comment {add, list, edit, delete}` — markdown bodies,
  edits show a unified diff in the preview
- `jc jira issue attachment {list, get, upload}` — `get` writes to disk
  and prints the path for Claude Code to read
- `jc jira issue link {list, add, remove, types}`
- `jc jira user {me, search}`
- `jc jira jql <query>` — raw JQL escape hatch, cursor-paginated
- `jc jira fields sync` — refreshes `~/.cache/jc/fields.json` so you can
  pass `--field "Story Points=5"` anywhere

**Confluence**
- `jc conf page {get, list, search, create, update, delete}` — markdown
  in, markdown out, via `body-format=atlas_doc_format`
- `jc conf space {list, get}`
- `jc conf attachment {list, get, upload}`
- `jc conf cql <query>` — raw CQL escape hatch

**Composite**
- `jc publish <md-file> --space <KEY> --title <...> [--parent <ID>] [--link-to <JIRA-KEY>]`
  — publish a markdown file as a Confluence page and drop a linking
  comment on a Jira ticket in a single preview-aware step

## Markdown-native rich content

Any command that takes a `--body-file`, `--description-file`, or
`--from-markdown` path treats the markdown as a first-class source and
handles the rich-content pieces that plain text can't express:

- **GFM tables** round-trip through the ADF converter on both sides,
  including inline marks in cells and backslash-safe cell escaping.
- **Local images** — `![diagram](./arch.png)` — are uploaded to the
  target (Jira issue, Confluence page) as attachments, and the
  markdown is rewritten to reference the resulting attachment ID
  before it hits the ADF converter. Relative paths resolve against
  the markdown file's parent directory. Dry-run previews the upload
  list without actually uploading; confirm mode uploads only after
  the user types `y` so cancellation never leaves orphans.
- **Typed mentions** — `@[alice@example.com]` or `@[Alice Smith]`
  or `@[accountId]` — are resolved via the Jira user search API and
  become real ADF mention inline nodes, so Confluence and Jira fire
  notifications as expected. Ambiguous matches error with the
  candidate list.
- **Exotic ADF nodes** (panels, status lozenges, expand blocks, layout
  sections, decision lists, etc.) round-trip losslessly through the
  ` ```adf:<type>` fenced-block escape hatch. Fence length auto-scales
  so nested backticks inside the serialized JSON can't break out.

## Global flags

- `--dry-run` — print the exact HTTP request as JSON, don't send
- `--confirm` — render preview to stderr, block on stdin y/N
- `--verbose` — trace HTTP method/URL/status to stderr, auth redacted,
  query strings on URLs replaced with `?<redacted>`
- `--limit N` — cap list/search results (0 = unlimited, default)
- `--show-query` — echo compiled JQL/CQL in `meta.query` for wrappers

## Output contract

- **stdout:** one JSON envelope per invocation — `{data, warnings[], meta}`
- **stderr:** structured error JSON with `status`, `code`, `messages[]`,
  `field_errors{}`
- **exit codes:** 0 success · 1 usage/io · 2 API error · 3 auth/config ·
  4 validation

## Rate limits

Atlassian rate-limits with `429 Too Many Requests` + a `Retry-After`
header. `jc` honors that automatically: every verb gets bounded retries
(up to 4 attempts) with exponential backoff when no `Retry-After` is
provided. GETs and downloads also retry on 502/503/504; mutations only
retry on 429 to avoid double-committing if a 5xx indicates partial
processing. If the server asks us to wait more than 120 seconds, we
give up and surface the 429 so the CLI doesn't block indefinitely.

## Project layout

Cargo workspace, five crates:

| Crate      | Purpose                                              |
| ---------- | ---------------------------------------------------- |
| `jc`       | The binary: CLI, config, preview, logging, image + mention pre-processors |
| `jc-core`  | Shared HTTP client, retry, auth, errors, cache       |
| `jc-adf`   | Pure markdown ↔ Atlassian Document Format converter  |
| `jc-jira`  | Jira Cloud REST v3 typed client                      |
| `jc-conf`  | Confluence Cloud REST v2 typed client                |

## Status

All commands listed above are implemented end to end. **95 unit tests**
across the workspace, all passing:

- `jc-adf` (34): to_adf, from_adf, GFM tables both sides, round-trip,
  `adf:<type>` escape hatch with auto-scaling fence length
- `jc-core` (19): retry policies and backoff, literal escaping, relative-
  time validation, URL scrubbing
- `jc-jira` (13): JQL builder + string escaping, transition fuzzy matcher
- `jc` binary (29): control-char sanitization, markdown image finder,
  markdown mention resolver

`cargo clippy --all-targets` is clean.

What's deliberately deferred: multi-site `--profile` switching, live
integration tests against a sandbox Atlassian site, generated table of
contents on the markdown → ADF write path (the TOC node round-trips
via the escape hatch in the meantime).

## Security

See [`SECURITY.md`](SECURITY.md) for the threat model and vulnerability
reporting policy. Highlights: server-controlled attachment filenames
are path-traversal-safe; `--confirm` and `--verbose` strip control
characters from server-sourced strings before writing to the TTY to
prevent ANSI escape injection; response bodies are bounded at 16 MiB
to prevent OOM attacks; redirect policy is explicitly
`Policy::limited(10)` with `https_only(true)` so reqwest's cross-
origin auth-stripping behavior is locked in.

## License

MIT. See [`LICENSE`](LICENSE).
