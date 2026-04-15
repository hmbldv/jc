# jc

**J**ira + **C**onfluence CLI, designed for Claude Code to consume.

A single Rust binary that turns Atlassian Cloud into a programmatic
surface: JSON-first output, markdown-native input, `--dry-run` on
every mutation, full CRUD parity with whatever the authenticated user
can do in the UI.

## Install

```sh
cargo install jc
```

Or from source:

```sh
cargo install --git https://github.com/hmbldv/jc
```

## Quickstart

```sh
# Configure (either env vars or keychain)
export JC_SITE=your-org.atlassian.net
export JC_EMAIL=you@example.com
export JC_TOKEN=...
# ...or: jc config set token -   (reads from stdin)

# Verify auth
jc config test

# Pull a ticket
jc jira issue get FOO-123

# Post a markdown comment (with images and mentions)
cat > /tmp/note.md <<'EOF'
# Investigation

cc @[alice@example.com]

![repro](./screenshot.png)
EOF
jc jira issue comment add FOO-123 --body-file /tmp/note.md --dry-run
jc jira issue comment add FOO-123 --body-file /tmp/note.md

# Publish a markdown file as a Confluence page and link it on a ticket
jc publish ./design.md --space ENG --title "Design" --link-to FOO-123
```

## Features

- **All Jira Cloud CRUD** — issues, comments, attachments, links,
  users, workflow transitions (with fuzzy matching), custom fields
  (with local name↔ID cache), JQL search
- **All Confluence Cloud CRUD** — pages, spaces, attachments, CQL
- **Composite `jc publish`** — markdown → Confluence → Jira link in
  one preview-aware step
- **Markdown-native rich content** — local images auto-upload,
  `@[user]` mentions resolve to real ADF mention nodes, GFM tables
  round-trip, exotic ADF nodes preserved via a lossless
  ` ```adf:<type>` escape hatch
- **Dry-run and confirm on every mutation** — previews include a
  unified diff on edits; cancelling confirm never orphans uploads
- **Bounded automatic retry** — 429 + `Retry-After` honoring, 5xx
  exponential backoff for reads, mutation-safe policy
- **Structured JSON output and errors** — designed for Claude Code
  to read and reason over

## Documentation

- [Full README and feature list](https://github.com/hmbldv/jc#readme)
- [`docs/CLAUDE.md`](https://github.com/hmbldv/jc/blob/main/docs/CLAUDE.md)
  — pattern-oriented reference for Claude Code
- [`docs/OVERVIEW.md`](https://github.com/hmbldv/jc/blob/main/docs/OVERVIEW.md)
  — design principles, architecture, fidelity matrix
- [`SECURITY.md`](https://github.com/hmbldv/jc/blob/main/SECURITY.md)
  — threat model and vulnerability reporting

## License

MIT — see [`LICENSE`](LICENSE).
