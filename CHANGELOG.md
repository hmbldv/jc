# Changelog

All notable changes to `jc` are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
