# jc-adf

Pure markdown ↔ [Atlassian Document Format](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/)
converter used by the [`jc`](https://github.com/hmbldv/jc) CLI.

## What round-trips

- Paragraphs and headings (H1–H6)
- Text marks: strong, em, code, strike
- Links
- Bullet and ordered lists (including GFM-style tight lists)
- Fenced code blocks with language hints
- Blockquotes, horizontal rules, hard breaks
- GFM tables (header + body, inline marks in cells; backslash and
  pipe escaping; column alignment is dropped because ADF doesn't
  model it)

## Read-only (ADF → markdown)

- `@user` mentions render as `@name`
- `mediaSingle` images render as `![alt](attachment:ID)` sidecar refs
- `inlineCard`, `emoji`

The `jc` binary layer runs pre-processors for the write direction of
mentions and local-image embedding, because those operations need
HTTP access which this pure crate doesn't have.

## Lossless escape hatch

Any ADF node type the converter doesn't explicitly handle becomes a
fenced code block whose info string is `adf:<type>` and whose body is
the raw node JSON:

````markdown
```adf:panel
{"type":"panel","attrs":{"panelType":"info"},"content":[
  {"type":"paragraph","content":[{"type":"text","text":"heads up"}]}
]}
```
````

On the reverse trip, `adf:*` fenced blocks re-inflate verbatim. The
fence length auto-scales based on the longest backtick run in the
serialized body so nested backticks can't break out of the escape.

## API

```rust
use jc_adf::{to_adf, to_markdown, AdfDocument};

// Markdown -> ADF
let adf: AdfDocument = to_adf("# Hello\n\n**world**");

// ADF -> Markdown
let md: String = to_markdown(&adf);
```

`AdfDocument` is a type alias for `serde_json::Value` so you can
inspect, mutate, and serialize with the rest of the `serde_json`
ecosystem.

## License

MIT — see [`LICENSE`](LICENSE).
