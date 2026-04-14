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

## Quickstart

Create an API token at <https://id.atlassian.com/manage-profile/security/api-tokens>,
then either export env vars:

```sh
export JC_SITE=your-org.atlassian.net
export JC_EMAIL=you@example.com
export JC_TOKEN=...
```

Or store them in the OS keychain:

```sh
jc config set site  your-org.atlassian.net
jc config set email you@example.com
jc config set token '...'
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
  edits show unified diff in preview
- `jc jira issue attachment {list, get, upload}` — get writes to disk
  and prints path for Claude Code to read
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

## Global flags

- `--dry-run` — print the exact HTTP request as JSON, don't send
- `--confirm` — render preview to stderr, block on stdin y/N
- `--verbose` — trace HTTP method/URL/status to stderr, auth redacted
- `--limit N` — cap list/search results (0 = unlimited, default)
- `--show-query` — echo compiled JQL/CQL in `meta.query` for wrappers

## Output contract

- **stdout:** one JSON envelope per invocation — `{data, warnings[], meta}`
- **stderr:** structured error JSON with `status`, `code`, `messages[]`,
  `field_errors{}`
- **exit codes:** 0 success · 1 usage/io · 2 API error · 3 auth/config ·
  4 validation

## Project layout

Cargo workspace, five crates:

| Crate      | Purpose                                              |
| ---------- | ---------------------------------------------------- |
| `jc`       | The binary: CLI, config, preview, logging            |
| `jc-core`  | Shared HTTP client, auth, errors, cache              |
| `jc-adf`   | Pure markdown ↔ Atlassian Document Format converter  |
| `jc-jira`  | Jira Cloud REST v3 typed client                      |
| `jc-conf`  | Confluence Cloud REST v2 typed client                |

## Status

All commands listed above are implemented end to end. The markdown↔ADF
converter is covered by 25 unit tests including round-trip and escape-hatch
cases; the JQL builder and transition matcher add another 16 tests. What's
deliberately deferred: automated 429 backoff, multi-site `--profile`
switching, live integration tests, and richer ADF elements in the converter
(GFM tables, generated TOC, typed user mentions on the write path, inline
image upload pipeline). Exotic ADF nodes round-trip losslessly via the
`adf:<type>` fenced-block escape hatch regardless of explicit support.

## License

MIT. See [`LICENSE`](LICENSE).
