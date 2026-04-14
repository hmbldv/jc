# jc — Overview

This document is the human-readable full scope of what `jc` is, why it exists,
and how it is put together. If you are Claude Code, read `CLAUDE.md` first;
this document is the supporting context for humans deciding whether and how
to use `jc`.

## 1. Purpose

`jc` is a single-binary CLI for Jira Cloud and Confluence Cloud, designed as a
first-class tool for Claude Code to consume. It exists because the available
options — the official Atlassian CLI, community wrappers, and various MCP
servers — all fell short in one of three ways:

- **Too limited** — missing CRUD parity with the UI, no good search, no
  attachments, no way to post rich-formatted comments.
- **Too opinionated** — forcing workflows that don't match how an autonomous
  agent actually needs to read and write.
- **Too fragile** — brittle JSON shapes, poor error surfaces, no dry-run,
  and no thought given to an LLM being the primary consumer.

The target workflow is simple to describe and hard to get right:

1. Claude Code pulls a Jira ticket and any linked Confluence docs.
2. Reasons over the ticket, the docs, and any attachments.
3. Drafts a plan.
4. Executes the code changes (in a separate tool — GitLab / GitHub — which is
   deliberately out of scope here).
5. Posts progress back to Jira: comments, transitions, status updates.
6. Validates the update landed correctly by reading the ticket again.

`jc` owns steps 1, 2, 5, and 6. It deliberately does not own step 4.

## 2. Design principles

**JSON-first output.** Every command emits a single JSON object on stdout.
Claude Code is the primary consumer. Humans can still read it, but they are
secondary.

**Markdown as input.** Users and agents write markdown. `jc` converts to ADF
(Atlassian Document Format) internally before sending. The markdown↔ADF
converter is the load-bearing piece of the whole tool.

**Dry-run by default for humans.** Every mutation supports `--dry-run`
(preview only) and `--confirm` (preview + stdin y/N). For Claude Code, the
pattern is: dry-run first, show the user, re-run for real after approval.

**No local state beyond pure cache.** The Atlassian API is authoritative. The
only local state is a small JSON cache for Jira custom field name↔ID mapping,
which is rebuildable at any time. No database, no spreadsheets, no background
sync, no offline mirror.

**Full CRUD parity with UI permissions.** Whatever the authenticated user can
do in the Atlassian UI, `jc` can do with their API token. No artificial
limitations.

**No MCP surface.** CLI only. Claude Code shells out, the way it does to
`gh` and `glab`. MCP has been a friction point; the CLI boundary is cleaner.

## 3. Architecture at a glance

Cargo workspace, single binary, five crates:

```
jc/
├── crates/
│   ├── jc/         # binary: clap CLI, config, preview/dry-run, logging
│   ├── jc-core/    # shared: reqwest client, auth, retry, errors, cache
│   ├── jc-adf/     # pure markdown <-> ADF converter
│   ├── jc-jira/    # Jira Cloud REST v3 typed client
│   └── jc-conf/    # Confluence Cloud REST v2 typed client
├── docs/
│   ├── CLAUDE.md       # pattern-oriented reference for Claude Code
│   ├── OVERVIEW.md     # this file
│   └── commands/       # per-command deep reference
└── tests/fixtures/     # recorded HTTP responses for replay tests
```

**Why workspace instead of single crate:**

- `jc-adf` is pure functions with no I/O. Isolating it in its own crate makes
  the tests fast, exhaustive, and property-testable.
- `jc-core` has no Atlassian-specific knowledge — the split enforces that
  boundary, preventing retry/auth/cache logic from leaking product assumptions.
- `jc-jira` and `jc-conf` are independent typed clients that share `jc-core`
  and `jc-adf`. Either could be replaced without touching the other.

## 4. The ADF problem and how we solved it

Jira Cloud's issue descriptions and comments are stored as **Atlassian
Document Format** — a nested JSON tree, like ProseMirror or TipTap. It is not
markdown, not HTML, and not wiki markup. Example:

```json
{
  "type": "doc",
  "version": 1,
  "content": [
    { "type": "paragraph", "content": [{ "type": "text", "text": "hi" }] }
  ]
}
```

Every rich element is its own node type: `codeBlock`, `table`, `mention`,
`inlineCard`, `panel`, `status`, `expand`, `layoutSection`, etc. Writing this
by hand is tedious. Reading it back as a human is worse.

Confluence Cloud has an even older format (XHTML-based "storage format") but
the v2 API supports ADF via `body-format=atlas_doc_format`. That means **one
markdown↔ADF converter can serve both products**.

### Fidelity guarantees

| Element | Behavior |
| --- | --- |
| Paragraphs, headings, emphasis, lists | Full round-trip |
| Links | Full round-trip; Jira-smart-link URLs preserved |
| Code blocks | Full fidelity, language hints preserved |
| GFM tables | Full round-trip, cell alignment preserved |
| Table of contents | Detected and regenerated as ADF TOC node |
| Inline images | Read as `![alt](attachment:name.png)` with sidecar attachment ID; write uploads first then references |
| User mentions | `@user` → async lookup → ADF `mention` with accountId; ambiguous matches error with candidates |
| Exotic nodes (panel, status, expand, layout, decisionList) | Rendered as fenced blocks with a type marker (e.g. ` ```adf:panel:info`) — lossless round-trip, nothing silently dropped |
| Non-inline attachments | Surfaced in `warnings[]` with a hint to run `attachment list` |

The "nothing silently dropped" rule is non-negotiable. If the converter can't
cleanly represent something, it escapes it to a fenced block and preserves
the raw ADF verbatim. That is what the `adf:*` type markers are for.

## 5. The dry-run / preview model

Every mutation command has three modes:

1. **`--dry-run`** — serializes the exact HTTP request (method, URL, redacted
   headers, body) as JSON, writes it to stdout, exits 0. No HTTP call.
2. **`--confirm`** — renders the preview, blocks on stdin for `y/N`, then
   sends. For interactive humans only.
3. **default** — sends it. For Claude Code acting with explicit upstream
   authorization.

The preview format is the same in all three modes, so Claude Code can always
do `--dry-run` first, show the user, and re-run without the flag once
confirmed.

**For edit operations**, the preview includes a unified diff between the
current remote state and the proposed new state, in addition to the raw ADF
payload. This is what makes "append my own considerations before pushing"
actually reviewable — the user sees what is changing in human terms, not as
an unreadable ADF tree diff.

## 6. Auth and config

**Primary:** env vars — `JC_SITE`, `JC_EMAIL`, `JC_TOKEN`. Simplest to wire up
for both Claude Code and CI.

**Fallback:** OS keychain via the `keyring` crate. For interactive use without
a `.env` file around.

**Verification:** `jc config test` calls `/rest/api/3/myself` and reports the
authenticated user. Run this first in any new session.

**Single site only.** Multi-site / profile support is reserved (`--profile`
flag exists in `--help`) but not implemented. When it becomes real, the
config becomes a profiles map and every command takes `--profile`.

## 7. Integration story

`jc` is one tool in a larger workflow. The full chain is:

```
Jira ticket  →  Confluence docs  →  Claude Code plan  →  GitLab PR  →  Jira update
     ^^^^^^^^^^^^^^^^^^^^^^^^^^                                          ^^^^^^^^^^^^^
              jc                                                              jc
```

`jc` owns the reads at the start and the writes at the end. GitLab is owned
by a separate tool (e.g. `glab`), deliberately. Mixing them into a single
binary would muddy the concerns and make both halves worse.

## 8. What is intentionally excluded

- **GitLab / GitHub integration** — separate tools already do this well.
- **MCP server mode** — CLI boundary is cleaner for the workflow we want.
- **Multi-site support** — deferred until it is actually needed.
- **Local mirror database** — the API is authoritative, no offline mode.
- **Spreadsheets or CSV dumps** — if you need these, build them on top of
  `jc jql` or `jc cql` output.
- **Background sync / polling** — `jc` is one-shot per invocation.
- **Undocumented Atlassian APIs** — everything uses the public REST surface.

## 9. Testing strategy

- **Unit tests** — exhaustive on `jc-adf` converters. Round-trip property
  tests where feasible. Pure functions make this cheap.
- **Recorded fixtures** — HTTP responses captured once, replayed in
  integration tests for `jc-jira` and `jc-conf`. Lives in `tests/fixtures/`.
- **Live integration tests** — deferred. Will run against a real sandbox
  Atlassian site once a second machine is available for isolation.

## 10. Toolchain

- Rust stable, edition 2024 (MSRV pinned via `rust-toolchain.toml`)
- `reqwest` with `rustls-tls` — no OpenSSL, no system libs
- `tokio` for async
- `clap` v4 with derive
- `serde` / `serde_json`
- `pulldown-cmark` for markdown parsing
- `keyring` for OS keychain
- `anyhow` at the binary boundary, `thiserror` in library crates
- `tracing` + `tracing-subscriber` for the `--verbose` HTTP log
