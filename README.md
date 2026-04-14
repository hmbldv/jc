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
See [`docs/CLAUDE.md`](docs/CLAUDE.md) for the pattern-oriented reference that
Claude Code reads when using the tool.

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
then export three env vars:

```sh
export JC_SITE=your-org.atlassian.net
export JC_EMAIL=you@example.com
export JC_TOKEN=...  # from the Atlassian token page
```

Verify auth:

```sh
jc config test
```

You should see a JSON object with your account id, display name, and active
status. If not, the error JSON on stderr will tell you what's wrong.

## Project layout

Cargo workspace with five crates:

| Crate      | Purpose                                               |
| ---------- | ----------------------------------------------------- |
| `jc`       | The binary: CLI, config, preview/dry-run, logging     |
| `jc-core`  | Shared HTTP client, auth, retry, errors, cache        |
| `jc-adf`   | Pure markdown ↔ Atlassian Document Format converter   |
| `jc-jira`  | Jira Cloud REST v3 typed client                       |
| `jc-conf`  | Confluence Cloud REST v2 typed client                 |

## Docs

- [`docs/OVERVIEW.md`](docs/OVERVIEW.md) — full scope, rationale, architecture
- [`docs/CLAUDE.md`](docs/CLAUDE.md) — pattern-oriented reference for Claude Code
- `docs/commands/` — per-command deep reference (populated as commands land)

## Status

Scaffold stage. `jc config test` is the first working command; the rest of
the tree is in place as stubs and lands endpoint-by-endpoint.

## License

MIT. See [`LICENSE`](LICENSE).
