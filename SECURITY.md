# Security Policy

## Reporting a Vulnerability

If you discover a security issue in `jc`, please **do not file a public
GitHub issue**. Instead, open a private advisory at
<https://github.com/hmbldv/jc/security/advisories/new> with:

- A description of the issue and its impact
- Steps to reproduce
- The `jc` version (`jc --version`) and commit SHA if installed from
  a git checkout
- Whether you'd like credit in the changelog

You can expect an initial acknowledgement within 72 hours. If the report
is confirmed, a fix and a coordinated disclosure timeline will follow.

## Supported Versions

`jc` is pre-1.0. Only the latest release is supported for security fixes
during this phase.

| Version | Supported |
| ------- | --------- |
| 0.1.x   | ✅        |
| < 0.1   | ❌        |

## Threat Model

`jc` is a client-side CLI tool. Its threat model treats the following as
trusted:

- The local user running the command
- The local filesystem and environment variables
- The OS keychain
- The Atlassian Cloud site identified by `JC_SITE`
- TLS PKI as implemented by `rustls` + `rustls-native-certs`

And the following as potentially hostile:

- **Server-controlled content** returned by the Atlassian API — issue
  summaries, comment bodies, page titles, attachment filenames, error
  messages. An attacker who can post a comment or set a title can
  influence these. `jc` sanitizes all such strings before rendering
  them to a TTY.
- **Attachment filenames** supplied by the server when writing downloads
  to disk. `jc` strips path components via `Path::file_name()`,
  rejects `.` / `..` / empty names, and refuses to overwrite a
  pre-existing symlink at the target path.
- **HTTP response bodies**. `jc` caps non-download response reads at
  16 MiB, streamed chunk-by-chunk so an unbounded body can't OOM the
  process. Attachment downloads are intentionally unbounded because
  the user explicitly asked for the bytes.
- **Cross-origin redirects** during attachment downloads. `jc` locks in
  `reqwest::redirect::Policy::limited(10)` with `https_only(true)`;
  reqwest's default behavior strips `Authorization` on cross-origin
  redirect, keeping the Atlassian basic auth out of the signed storage
  URL.
- **Rate-limit backoff storms**. `jc` bounds retry at 4 attempts per
  request and 120 seconds per wait, so a hostile or misconfigured
  server sending `Retry-After: 86400` can't block the CLI
  indefinitely — the retry loop gives up and surfaces the 429 so
  the user can rerun manually.

The image upload pre-processor reads local files from the markdown
source's parent directory (or absolute paths), resolving `![](path)`
references the user wrote. There is intentionally no filesystem
sandbox on the read side — the trust boundary is "the user
explicitly chose this markdown file, so they are responsible for its
contents." If Claude Code is running with a markdown file from an
untrusted source, the user should inspect the file before running
a `--body-file` / `--from-markdown` command.

Out of scope:

- A compromised Atlassian account with legitimate write access can
  obviously write data via `jc` — that's not a vulnerability in `jc`,
  it's the user's account credentials being compromised.
- A compromised local machine can read the keychain and env vars. That
  is the OS's concern.

## Hardening Overview

See the `[0.1.0]` entry in [`CHANGELOG.md`](CHANGELOG.md) for the full
list of security measures shipped in the initial release, including
path-traversal defenses, terminal-escape sanitization, CQL/JQL
injection fixes, response-body bounds, explicit redirect policy, and
keyring namespace. The `[Unreleased]` section documents additions
since that release (retry circuit breaker, image upload pre-processor,
mention resolver, GFM table + escape-hatch integrity fixes).
