# jc-conf

Typed client for [Confluence Cloud REST v2](https://developer.atlassian.com/cloud/confluence/rest/v2/).
Used by the [`jc`](https://github.com/hmbldv/jc) CLI.

## Coverage

- **Pages** — `get` (body rendered as ADF via
  `body-format=atlas_doc_format`), `list` (with `--parent` children
  filter and v2 cursor pagination via `_links.next`), `create`,
  `update`, `delete`, plus a `BodyRequest::from_adf` helper that
  handles the v2 envelope's double-serialized ADF string quirk.
- **Spaces** — `list`, `get`, `find_by_key`, `resolve_id` (for
  transparent `--space ENG` → numeric spaceId resolution).
- **Attachments** — `get_meta` (v2), `list_on_page` (v2 with
  pagination), `download` (follows the attachment's `downloadLink`),
  `upload` (still uses the v1 multipart endpoint because v2 doesn't
  cover attachment upload yet).
- **Search** — `cql` against `/wiki/rest/api/search` with old-style
  start/limit pagination, since CQL hasn't been modernized to v2.

All HTTP goes through [`jc-core`](https://crates.io/crates/jc-core);
markdown ↔ ADF conversion is delegated to
[`jc-adf`](https://crates.io/crates/jc-adf).

## Usage

```rust
use jc_core::Client;
use reqwest::Url;

let client = Client::new(
    Url::parse("https://your-org.atlassian.net/")?,
    "you@example.com".to_string(),
    "<api-token>".to_string(),
)?;

// Fetch a page with body as ADF
let page = jc_conf::page::get(&client, "12345").await?;
if let Some(adf) = page.body.and_then(|b| b.as_adf()) {
    let md = jc_adf::to_markdown(&adf);
    println!("{md}");
}

// Resolve a space key to its numeric ID
let space_id = jc_conf::space::resolve_id(&client, "ENG").await?;

// Create a new page from a markdown-derived ADF tree
let adf = jc_adf::to_adf("# Design\n\nDetails follow...");
let req = jc_conf::page::CreatePageRequest {
    space_id: &space_id,
    status: "current",
    title: "My Page",
    parent_id: None,
    body: jc_conf::page::BodyRequest::from_adf(&adf),
};
let page = jc_conf::page::create(&client, &req).await?;
```

## License

MIT — see [`LICENSE`](LICENSE).
