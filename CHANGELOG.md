# Changelog

All notable changes to `jc` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **BREAKING BEHAVIORAL FIX — `jira issue link add` direction inversion.**
  In 0.1.0, `jc jira issue link add <KEY> --type Blocks --to <OTHER>` sent
  `inwardIssue`/`outwardIssue` swapped relative to Atlassian's REST v3
  convention, producing the inverse link direction in Jira (e.g. "KEY is
  blocked by OTHER" instead of the documented "KEY blocks OTHER"). The bug
  affected every directional link type (`Blocks`, `Duplicate`, `Clones`,
  …); `Relates` is symmetric so no user-visible change there. The payload
  assignments are now flipped to match the CLI help text, and the
  library-level `jc_jira::issue_links::add` parameters have been renamed
  from `outward_key`/`inward_key` to `from_key`/`to_key` to prevent the
  same misreading recurring. A unit-test snapshot of the outgoing JSON
  locks in the correct orientation.

  **Migration:** links created with 0.1.0 using a directional type point
  the opposite direction of the operator's intent. Audit with
  `jc jira issue link list <KEY> --json` and recreate any links whose
  direction matters for dashboards, JQL (`linkedIssues(X, "blocks")`),
  or automations.

### Added

- **Automatic retry with bounded backoff** (`jc-core::retry`). Every
  HTTP verb routes through a `send_with_retry` helper that honors
  `Retry-After` on 429 responses, falls back to exponential backoff
  (500 ms → 1 s → 2 s → 4 s, capped ~64 s) when no header is
  provided, gives up after 4 attempts, and refuses to block longer
  than 120 s on a single response so the CLI can't stall
  indefinitely.

  Retry policy is per-verb:
  - `RetryPolicy::Read` (GET / HEAD / download): 429 + 502 + 503 + 504
  - `RetryPolicy::IdempotencySafe` (POST / PUT / DELETE / patch-less
    204 endpoints): 429 only — other 5xx responses might indicate the
    server partially processed a mutation, and we'd rather surface
    the error than double-commit
  - `RetryPolicy::None` (multipart upload): the `reqwest::multipart::
    Form` is move-consumed on send, so retrying would require
    re-reading the source file; single attempt only.

- **Local image upload pre-processor** (`jc::markdown_images`).
  `![alt](./diagram.png)` in any body-file / description-file /
  from-markdown input now uploads the referenced file as an
  attachment on the correct target and rewrites the markdown to
  reference the resulting attachment ID before conversion. Relative
  paths resolve against the markdown file's parent directory. Each
  unique URL is uploaded once even when referenced multiple times.

  - Edit commands (`comment add`, `comment edit`, `issue edit`,
    `conf page update`) upload only *after* the `--confirm` gate so
    cancellation leaves no orphaned attachments.
  - Create commands (`issue create`, `page create`, `publish`) run
    two-phase: create the target with the markdown unchanged, upload
    images to the newly-created target, follow up with an edit that
    rewrites the body to reference real attachment IDs. Partial
    failure on the follow-up surfaces as a `warnings[]` entry
    rather than hard-erroring so the created target isn't orphaned.
  - Dry-run mode lists pending uploads in `warnings[]` without
    actually uploading anything.

- **Typed `@mention` resolver** (`jc::markdown_mentions`). `@[query]`
  tokens in any markdown body are resolved via Jira user search and
  become real ADF `mention` inline nodes so the target product
  notifies the user and renders a clickable mention instead of a
  literal `@alice` text.

  Query forms accepted:
  - Long alphanumeric accountId — used directly, no API round-trip
  - Email address — resolved by exact case-insensitive match
  - Display name or substring — exact match wins over partial; a
    single partial match is accepted; multiple partial matches
    without a tiebreaker error out with the candidate list for
    disambiguation.

  Resolution runs on all seven write-path commands
  (`comment add/edit`, `issue create/edit`, `conf page
  create/update`, `publish`). Mentions inside marked text (bold,
  italic, code) are intentionally left as literal text because ADF
  mention nodes don't support marks; splitting a marked text run
  would silently drop formatting.

- **GFM table support on both directions of the ADF converter.**
  Markdown → ADF emits `table` / `tableRow` / `tableHeader` /
  `tableCell` nodes with inline marks preserved inside cells. ADF
  → Markdown walks rows, detects the header row by checking whether
  all cells are `tableHeader`, synthesizes an empty header when
  absent, emits a left-aligned separator, and escapes pipes and
  backslashes inside cell content. Column alignment from GFM input
  is dropped because ADF doesn't model it.

- **Escape-hatch fence length now auto-scales.** `render_unknown_block`
  and `render_unknown_inline` pick a fence with more backticks than
  the longest run inside the serialized JSON body so a node whose
  string value contains triple backticks can no longer break out of
  the escape hatch on a round-trip.

### Changed

- **Stub cleanup.** Eight never-implemented placeholder modules
  (`jc-core::retry`, `jc-core::paginate`, `jc-adf::toc`,
  `jc-adf::attachments`, `jc-adf::mentions`, `jc-adf::tables`,
  `jc-adf::unknown`, `jc-conf::types`) were originally deleted as
  dead code, then three of them were reimplemented for real:
  `retry` (now the shared retry layer), `attachments` and `mentions`
  (now inlined as preprocessor modules in the `jc` crate because
  they need HTTP access, which `jc-adf` is forbidden from having).
  `jc-adf::lib.rs` docstring rewritten to honestly describe what
  round-trips and what's deferred.

### Fixed

- **`escape_table_cell` now escapes backslash**. A cell containing
  the literal text `\|` previously rendered as `\\|` in the output,
  which GFM parses as `\` followed by a cell terminator — the pipe
  was lost and cell boundaries shifted. Fix: escape `\` → `\\`
  before `|` → `\|`, matching the order `jc_core::literal::
  escape_string` uses.

## [0.1.0] — 2026-04-14

Initial public release.

### Added

- **Jira Cloud REST v3 client** (`jc-jira` crate):
  - Issue operations: get, create, edit, list, mine, search, transition,
    assign, watch/unwatch
  - Comment operations: add, list, edit (with diff preview), delete
  - Attachment operations: list, download, upload
  - Issue link operations: list, add, remove, types
  - User operations: me, search
  - Custom field name↔ID resolution with local cache (`jc jira fields sync`)
  - Raw JQL escape hatch with cursor auto-pagination against the new
    `POST /rest/api/3/search/jql` endpoint
  - JQL builder with typed literal escaping and relative-time validation
  - Fuzzy workflow transition matcher

- **Confluence Cloud REST v2 client** (`jc-conf` crate):
  - Page operations: get, list, search, create, update (with diff
    preview), delete — via `body-format=atlas_doc_format`
  - Space operations: list, get, find-by-key
  - Attachment operations: list, download, upload
  - CQL search via the v1 endpoint (not yet modernized to v2)

- **Composite commands**:
  - `jc publish <md> --space --title [--parent] [--link-to]` —
    markdown → Confluence page → Jira linking comment in a single
    preview-aware step

- **ADF converter** (`jc-adf` crate):
  - Markdown → ADF and ADF → Markdown covering paragraphs, headings,
    marks (strong, em, code, strike, link), lists (including tight
    lists), code blocks with language hints, blockquotes, rules,
    hard breaks, mentions (read), emoji, inline cards, and
    `mediaSingle` images (read as `![alt](attachment:ID)`).
  - Lossless round-trip escape hatch: any unrecognized ADF node is
    rendered as a ` ```adf:<type>` fenced code block whose body is the
    raw node JSON and re-inflates verbatim on the reverse trip.

- **HTTP infrastructure** (`jc-core` crate):
  - Basic auth Atlassian client with all common verbs
  - Explicit `Policy::limited(10)` redirect policy and `https_only(true)`
    — attachment downloads can follow the Atlassian 303 → signed storage
    flow safely (reqwest strips `Authorization` on cross-origin redirect)
  - Bounded response body reads (16 MiB cap) for JSON and error bodies
  - Attachment downloads are intentionally unbounded
  - Multipart upload with `X-Atlassian-Token: no-check`
  - Structured error envelope parsing

- **Config and auth**:
  - Environment variables (`JC_SITE`, `JC_EMAIL`, `JC_TOKEN`) with
    OS keychain fallback via the `keyring` crate
  - `jc config set <key> <value>` to store secrets in the keychain;
    pass `-` as the value to read from stdin and keep secrets out of
    shell history
  - Namespaced keyring service `dev.hmbldv.jc`

- **Preview / dry-run / confirm harness**:
  - `--dry-run` emits the exact planned HTTP request as JSON on stdout
  - `--confirm` renders the preview to stderr and blocks on stdin y/N
  - Composite commands emit `{previews: [...]}` with step count
  - Edit operations fetch current state and show a unified diff

- **CLI infrastructure**:
  - `--verbose` HTTP request/response tracing (method, status, URL with
    query string redacted) via `tracing`
  - Structured JSON error envelope on stderr with typed exit codes
    (1 usage/io, 2 api, 3 config/auth, 4 validation)
  - JSON envelope on stdout with `data`, `warnings[]`, `meta`

### Security

- **Path traversal hardening**: attachment downloads extract the server-
  supplied filename with `Path::file_name()` before joining the output
  directory, rejecting `../`, absolute paths, and reserved names
  (`.`, `..`, empty). Refuses to overwrite pre-planted symlinks.
- **Terminal escape sanitization**: `--confirm` and `--verbose` output
  pass server-sourced strings through a control-character filter
  (strips C0/C1 and DEL, keeps `\n` and `\t`) to prevent ANSI-escape
  injection from Jira summaries / Confluence titles / diff content
  rewriting the terminal or faking a confirmation.
- **CQL injection fix**: the `conf page search` wrapper now shares the
  same `literal::escape_string` helper as JQL, escaping both `\` and
  `"`. The previous inline `"` replacement missed backslash and allowed
  string-literal escape.
- **JQL `--updated` validation**: the `issue list --updated` flag is
  now validated against a `[+-]?\d+[smhdwMy]` allow-list before being
  interpolated into JQL, eliminating a raw-clause injection path.
- **Verbose log URL redaction**: query strings in logged URLs are
  replaced with `?<redacted>` so signed-URL redirect targets can't
  leak credentials through `--verbose`.
- **Response body cap**: 16 MiB limit on JSON and error bodies, read
  chunk-by-chunk via `Response::chunk` so a hostile endpoint can't OOM
  the process with an unbounded chunked body.
- **Non-TTY confirm rejection**: `--confirm` errors immediately when
  stdin is not a terminal, instead of silently declining every prompt.
- **Redirect policy lock-in**: explicit `Policy::limited(10)` and
  `https_only(true)` prevent a future reqwest default change from
  silently weakening the attachment download flow.

[Unreleased]: https://github.com/hmbldv/jc/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hmbldv/jc/releases/tag/v0.1.0
