# jc-jira

Typed client for [Jira Cloud REST v3](https://developer.atlassian.com/cloud/jira/platform/rest/v3/).
Used by the [`jc`](https://github.com/hmbldv/jc) CLI.

## Coverage

- **Issues** — `get`, `create`, `edit`, `assign`, `add_watcher` /
  `remove_watcher`
- **Comments** — `add`, `get`, `list`, `edit`, `delete`
- **Attachments** — `get_meta`, `download`, `upload`
- **Issue links** — `list_types`, `list_on_issue`, `add`, `remove`
- **Workflow transitions** — `list`, `execute`, plus a `find_match`
  fuzzy matcher (exact case-insensitive match wins over substring;
  ambiguous partial matches return the full candidate list)
- **Users** — `myself`, `search`
- **Fields** — `list_all` and a `FieldsCache` that serializes to
  `~/.cache/jc/fields.json` for custom field name↔ID resolution
- **JQL search** — `jql(client, query, fields, limit)` with the new
  `POST /rest/api/3/search/jql` cursor-paginated endpoint
- **JQL builder** — `JqlBuilder` with `eq` / `contains` / `raw` /
  `order_by` and correctly-escaped literals

All HTTP goes through [`jc-core`](https://crates.io/crates/jc-core),
which handles basic auth, retry (429 + Retry-After, exponential
backoff, per-verb policy), bounded response bodies, and cross-origin
auth stripping on redirects.

Markdown ↔ ADF conversion is delegated to
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

// Fetch an issue with all fields
let issue = jc_jira::issue::get(&client, "FOO-123").await?;

// Post a comment (body is ADF; use jc_adf::to_adf to build it)
let adf = jc_adf::to_adf("Great work! cc @alice");
jc_jira::comment::add(&client, "FOO-123", &adf).await?;

// Fuzzy-transition
let transitions = jc_jira::transitions::list(&client, "FOO-123").await?;
if let jc_jira::transitions::MatchResult::Unique(t) =
    jc_jira::transitions::find_match(&transitions, "In Review")
{
    jc_jira::transitions::execute(&client, "FOO-123", &t.id).await?;
}
```

## License

MIT — see [`LICENSE`](LICENSE).
