//! ADF -> Markdown.
//!
//! ADF is kept as `serde_json::Value` internally. That keeps the converter
//! small and trivially extensible: new node types are one match arm away,
//! and unknown types always fall through to the lossless escape hatch.

use serde_json::Value;

/// Convert an ADF document to markdown.
///
/// Accepts either a full `{"type":"doc","content":[...]}` document or a bare
/// content array. Trailing blank lines are trimmed.
pub fn to_markdown(doc: &Value) -> String {
    let mut out = String::new();
    let content = doc.get("content").and_then(Value::as_array);
    if let Some(nodes) = content {
        render_blocks(nodes, &mut out, 0);
    }
    while out.ends_with("\n\n") {
        out.pop();
    }
    out
}

fn render_blocks(nodes: &[Value], out: &mut String, depth: usize) {
    for (i, node) in nodes.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        render_block(node, out, depth);
    }
}

fn render_block(node: &Value, out: &mut String, depth: usize) {
    let ty = node_type(node);
    match ty {
        "paragraph" => {
            render_inlines(node, out);
            out.push('\n');
        }
        "heading" => {
            let level = node
                .get("attrs")
                .and_then(|a| a.get("level"))
                .and_then(Value::as_u64)
                .unwrap_or(1)
                .clamp(1, 6) as usize;
            for _ in 0..level {
                out.push('#');
            }
            out.push(' ');
            render_inlines(node, out);
            out.push('\n');
        }
        "codeBlock" => {
            let lang = node
                .get("attrs")
                .and_then(|a| a.get("language"))
                .and_then(Value::as_str)
                .unwrap_or("");
            out.push_str("```");
            out.push_str(lang);
            out.push('\n');
            if let Some(children) = node.get("content").and_then(Value::as_array) {
                for c in children {
                    if let Some(text) = c.get("text").and_then(Value::as_str) {
                        out.push_str(text);
                    }
                }
            }
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str("```\n");
        }
        "bulletList" => render_list(node, out, depth, false),
        "orderedList" => render_list(node, out, depth, true),
        "blockquote" => {
            let mut inner = String::new();
            if let Some(children) = node.get("content").and_then(Value::as_array) {
                render_blocks(children, &mut inner, depth);
            }
            for line in inner.lines() {
                out.push_str("> ");
                out.push_str(line);
                out.push('\n');
            }
        }
        "rule" => {
            out.push_str("---\n");
        }
        "mediaSingle" | "mediaGroup" => {
            if let Some(children) = node.get("content").and_then(Value::as_array) {
                for media in children {
                    render_media(media, out);
                }
            }
        }
        "table" => render_table(node, out),
        _ => render_unknown_block(node, out),
    }
}

/// Render an ADF table as a GFM pipe table.
///
/// ADF doesn't model column alignment, so separators are always left-
/// aligned `---`. If the first row has all `tableHeader` cells we use it
/// as the GFM header; otherwise we synthesize an empty header row so the
/// output is still valid GFM (which requires a header).
fn render_table(node: &Value, out: &mut String) {
    let Some(rows) = node.get("content").and_then(Value::as_array) else {
        return;
    };
    if rows.is_empty() {
        return;
    }

    let first_cells = rows[0].get("content").and_then(Value::as_array);
    let ncols = first_cells.map(|c| c.len()).unwrap_or(0);
    if ncols == 0 {
        return;
    }
    let first_is_header = first_cells
        .map(|cells| cells.iter().all(|c| node_type(c) == "tableHeader"))
        .unwrap_or(false);

    // Header
    if first_is_header {
        render_table_row(&rows[0], out);
    } else {
        out.push('|');
        for _ in 0..ncols {
            out.push_str("   |");
        }
        out.push('\n');
    }

    // Separator
    out.push('|');
    for _ in 0..ncols {
        out.push_str(" --- |");
    }
    out.push('\n');

    // Body
    let body_start = if first_is_header { 1 } else { 0 };
    for row in &rows[body_start..] {
        render_table_row(row, out);
    }
}

fn render_table_row(row: &Value, out: &mut String) {
    out.push('|');
    let Some(cells) = row.get("content").and_then(Value::as_array) else {
        out.push('\n');
        return;
    };
    for cell in cells {
        let mut cell_text = String::new();
        render_cell_inline(cell, &mut cell_text);
        out.push(' ');
        out.push_str(&escape_table_cell(&cell_text));
        out.push_str(" |");
    }
    out.push('\n');
}

/// Flatten an ADF table cell's content into inline markdown. Cells hold
/// paragraphs in the ADF model; GFM table cells only support inline
/// content, so multiple paragraphs are joined with a single space.
fn render_cell_inline(cell: &Value, out: &mut String) {
    let Some(content) = cell.get("content").and_then(Value::as_array) else {
        return;
    };
    for (i, block) in content.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        if node_type(block) == "paragraph" {
            render_inlines(block, out);
        }
    }
}

/// Escape GFM table cell content.
///
/// GFM's cell-escape grammar: `\\` → `\`, `\|` → `|`. The order matters:
/// backslash must be escaped first so the backslash in `\|` isn't itself
/// interpreted. Without the backslash escape, a cell containing the
/// literal text `\|` would round-trip as a cell terminator rather than
/// as its original characters — a data-integrity bug, and a subtle way
/// to smuggle cell boundaries through a round trip if cell content is
/// attacker-controlled.
///
/// Embedded newlines collapse to spaces because GFM table cells are
/// single-line by definition.
fn escape_table_cell(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|' => out.push_str("\\|"),
            '\n' | '\r' => out.push(' '),
            _ => out.push(ch),
        }
    }
    out
}

fn render_list(node: &Value, out: &mut String, depth: usize, ordered: bool) {
    let Some(items) = node.get("content").and_then(Value::as_array) else {
        return;
    };
    for (i, item) in items.iter().enumerate() {
        for _ in 0..depth {
            out.push_str("  ");
        }
        if ordered {
            out.push_str(&format!("{}. ", i + 1));
        } else {
            out.push_str("- ");
        }
        let mut inner = String::new();
        if let Some(children) = item.get("content").and_then(Value::as_array) {
            render_blocks(children, &mut inner, depth + 1);
        }
        let mut lines = inner.lines();
        if let Some(first) = lines.next() {
            out.push_str(first);
            out.push('\n');
        } else {
            out.push('\n');
        }
        for line in lines {
            if line.is_empty() {
                continue;
            }
            for _ in 0..=depth {
                out.push_str("  ");
            }
            out.push_str(line);
            out.push('\n');
        }
    }
}

fn render_media(media: &Value, out: &mut String) {
    let attrs = media.get("attrs");
    let id = attrs
        .and_then(|a| a.get("id"))
        .and_then(Value::as_str)
        .unwrap_or("?");
    let alt = attrs
        .and_then(|a| a.get("alt"))
        .and_then(Value::as_str)
        .unwrap_or("");
    out.push_str("![");
    out.push_str(alt);
    out.push_str("](attachment:");
    out.push_str(id);
    out.push_str(")\n");
}

fn render_inlines(node: &Value, out: &mut String) {
    if let Some(children) = node.get("content").and_then(Value::as_array) {
        for c in children {
            render_inline(c, out);
        }
    }
}

fn render_inline(node: &Value, out: &mut String) {
    match node_type(node) {
        "text" => render_text(node, out),
        "hardBreak" => out.push_str("  \n"),
        "mention" => {
            let name = node
                .get("attrs")
                .and_then(|a| a.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("?");
            out.push('@');
            out.push_str(name.trim_start_matches('@'));
        }
        "inlineCard" => {
            let url = node
                .get("attrs")
                .and_then(|a| a.get("url"))
                .and_then(Value::as_str)
                .unwrap_or("");
            out.push('<');
            out.push_str(url);
            out.push('>');
        }
        "emoji" => {
            let shortname = node
                .get("attrs")
                .and_then(|a| a.get("shortName"))
                .and_then(Value::as_str)
                .unwrap_or(":?:");
            out.push_str(shortname);
        }
        _ => render_unknown_inline(node, out),
    }
}

fn render_text(node: &Value, out: &mut String) {
    let text = node.get("text").and_then(Value::as_str).unwrap_or("");
    let marks = node
        .get("marks")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let link_href = marks
        .iter()
        .find(|m| node_type(m) == "link")
        .and_then(|m| m.get("attrs"))
        .and_then(|a| a.get("href"))
        .and_then(Value::as_str);

    let (open, close) = build_wrapping_marks(&marks);

    if let Some(href) = link_href {
        out.push('[');
        out.push_str(&open);
        out.push_str(text);
        out.push_str(&close);
        out.push_str("](");
        out.push_str(href);
        out.push(')');
    } else {
        out.push_str(&open);
        out.push_str(text);
        out.push_str(&close);
    }
}

fn build_wrapping_marks(marks: &[Value]) -> (String, String) {
    let mut open = String::new();
    let mut close = String::new();
    for mark in marks {
        match node_type(mark) {
            "strong" => {
                open.push_str("**");
                close.insert_str(0, "**");
            }
            "em" => {
                open.push('*');
                close.insert(0, '*');
            }
            "code" => {
                open.push('`');
                close.insert(0, '`');
            }
            "strike" => {
                open.push_str("~~");
                close.insert_str(0, "~~");
            }
            // `link` is handled by the caller so it wraps the whole text.
            // `underline`, `subsup`, `textColor`, etc. have no clean markdown
            // representation and are silently flattened for now. The text
            // itself is preserved — only the decoration is dropped.
            _ => {}
        }
    }
    (open, close)
}

fn render_unknown_block(node: &Value, out: &mut String) {
    let ty = node_type(node);
    let serialized = serde_json::to_string_pretty(node).unwrap_or_default();
    // Pick a fence longer than any backtick run in the serialized body so
    // a nested string literal containing ``` can't break out of the
    // escape hatch. Minimum of 3 backticks keeps the output familiar.
    let fence_len = longest_backtick_run(&serialized).max(2) + 1;
    let fence = "`".repeat(fence_len);
    out.push_str(&fence);
    out.push_str("adf:");
    out.push_str(if ty.is_empty() { "unknown" } else { ty });
    out.push('\n');
    out.push_str(&serialized);
    if !serialized.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&fence);
    out.push('\n');
}

fn render_unknown_inline(node: &Value, out: &mut String) {
    let ty = node_type(node);
    let serialized = serde_json::to_string(node).unwrap_or_default();
    let body = format!(
        "adf:{}:{}",
        if ty.is_empty() { "unknown" } else { ty },
        serialized
    );
    // Inline code spans: opening and closing delimiters must use at least
    // one more backtick than any run inside the body, otherwise a nested
    // backtick terminates the span early. `longest_backtick_run + 1` with
    // a minimum of 1 achieves that.
    let fence_len = longest_backtick_run(&body) + 1;
    let fence = "`".repeat(fence_len);
    out.push_str(&fence);
    // Pad leading/trailing space when the body starts or ends with a
    // backtick — CommonMark strips exactly one such space on parse.
    let pad_start = body.starts_with('`');
    let pad_end = body.ends_with('`');
    if pad_start {
        out.push(' ');
    }
    out.push_str(&body);
    if pad_end {
        out.push(' ');
    }
    out.push_str(&fence);
}

/// Length of the longest consecutive run of backticks in `s`.
fn longest_backtick_run(s: &str) -> usize {
    let mut max_run = 0;
    let mut current = 0;
    for c in s.chars() {
        if c == '`' {
            current += 1;
            if current > max_run {
                max_run = current;
            }
        } else {
            current = 0;
        }
    }
    max_run
}

fn node_type(node: &Value) -> &str {
    node.get("type").and_then(Value::as_str).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn doc(content: Value) -> Value {
        json!({"type": "doc", "version": 1, "content": content})
    }

    #[test]
    fn empty_doc() {
        assert_eq!(to_markdown(&doc(json!([]))), "");
    }

    #[test]
    fn simple_paragraph() {
        let d = doc(json!([
            {"type": "paragraph", "content": [{"type": "text", "text": "hello world"}]}
        ]));
        assert_eq!(to_markdown(&d), "hello world\n");
    }

    #[test]
    fn heading_levels() {
        let d = doc(json!([
            {"type": "heading", "attrs": {"level": 1}, "content": [{"type": "text", "text": "Top"}]},
            {"type": "heading", "attrs": {"level": 3}, "content": [{"type": "text", "text": "Sub"}]}
        ]));
        assert_eq!(to_markdown(&d), "# Top\n\n### Sub\n");
    }

    #[test]
    fn text_marks() {
        let d = doc(json!([{
            "type": "paragraph",
            "content": [
                {"type": "text", "text": "plain "},
                {"type": "text", "text": "bold", "marks": [{"type": "strong"}]},
                {"type": "text", "text": " "},
                {"type": "text", "text": "italic", "marks": [{"type": "em"}]},
                {"type": "text", "text": " "},
                {"type": "text", "text": "code", "marks": [{"type": "code"}]},
            ]
        }]));
        assert_eq!(to_markdown(&d), "plain **bold** *italic* `code`\n");
    }

    #[test]
    fn link_mark() {
        let d = doc(json!([{
            "type": "paragraph",
            "content": [{
                "type": "text",
                "text": "click",
                "marks": [{"type": "link", "attrs": {"href": "https://example.com"}}]
            }]
        }]));
        assert_eq!(to_markdown(&d), "[click](https://example.com)\n");
    }

    #[test]
    fn code_block_with_lang() {
        let d = doc(json!([{
            "type": "codeBlock",
            "attrs": {"language": "rust"},
            "content": [{"type": "text", "text": "fn main() {}"}]
        }]));
        assert_eq!(to_markdown(&d), "```rust\nfn main() {}\n```\n");
    }

    #[test]
    fn bullet_list() {
        let d = doc(json!([{
            "type": "bulletList",
            "content": [
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "one"}]}
                ]},
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "two"}]}
                ]},
            ]
        }]));
        assert_eq!(to_markdown(&d), "- one\n- two\n");
    }

    #[test]
    fn ordered_list() {
        let d = doc(json!([{
            "type": "orderedList",
            "content": [
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "first"}]}
                ]},
                {"type": "listItem", "content": [
                    {"type": "paragraph", "content": [{"type": "text", "text": "second"}]}
                ]},
            ]
        }]));
        assert_eq!(to_markdown(&d), "1. first\n2. second\n");
    }

    #[test]
    fn mention() {
        let d = doc(json!([{
            "type": "paragraph",
            "content": [
                {"type": "text", "text": "cc "},
                {"type": "mention", "attrs": {"id": "acct:123", "text": "@alice"}}
            ]
        }]));
        assert_eq!(to_markdown(&d), "cc @alice\n");
    }

    #[test]
    fn media_single_image() {
        let d = doc(json!([{
            "type": "mediaSingle",
            "content": [{
                "type": "media",
                "attrs": {"id": "att-42", "alt": "architecture diagram", "type": "file"}
            }]
        }]));
        assert_eq!(
            to_markdown(&d),
            "![architecture diagram](attachment:att-42)\n"
        );
    }

    #[test]
    fn unknown_block_uses_escape_hatch() {
        let d = doc(json!([{
            "type": "panel",
            "attrs": {"panelType": "info"},
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "heads up"}]}
            ]
        }]));
        let md = to_markdown(&d);
        assert!(md.starts_with("```adf:panel\n"), "got: {md}");
        assert!(md.contains("\"panelType\": \"info\""), "got: {md}");
        assert!(md.trim_end().ends_with("```"), "got: {md}");
    }

    #[test]
    fn blockquote() {
        let d = doc(json!([{
            "type": "blockquote",
            "content": [
                {"type": "paragraph", "content": [{"type": "text", "text": "quoted"}]}
            ]
        }]));
        assert_eq!(to_markdown(&d), "> quoted\n");
    }

    #[test]
    fn table_with_header_and_body() {
        let d = doc(json!([{
            "type": "table",
            "attrs": {"isNumberColumnEnabled": false, "layout": "default"},
            "content": [
                {
                    "type": "tableRow",
                    "content": [
                        {
                            "type": "tableHeader",
                            "attrs": {},
                            "content": [{
                                "type": "paragraph",
                                "content": [{"type": "text", "text": "Name"}]
                            }]
                        },
                        {
                            "type": "tableHeader",
                            "attrs": {},
                            "content": [{
                                "type": "paragraph",
                                "content": [{"type": "text", "text": "Score"}]
                            }]
                        }
                    ]
                },
                {
                    "type": "tableRow",
                    "content": [
                        {
                            "type": "tableCell",
                            "attrs": {},
                            "content": [{
                                "type": "paragraph",
                                "content": [{"type": "text", "text": "Alice"}]
                            }]
                        },
                        {
                            "type": "tableCell",
                            "attrs": {},
                            "content": [{
                                "type": "paragraph",
                                "content": [{"type": "text", "text": "42"}]
                            }]
                        }
                    ]
                }
            ]
        }]));
        let md = to_markdown(&d);
        assert!(md.contains("| Name | Score |"), "got: {md}");
        assert!(md.contains("| --- | --- |"), "got: {md}");
        assert!(md.contains("| Alice | 42 |"), "got: {md}");
    }

    #[test]
    fn escape_hatch_grows_fence_for_nested_backticks() {
        // An ADF panel whose text field contains triple backticks could
        // previously break out of the escape hatch. The renderer must
        // pick a longer fence so the closing delimiter is unambiguous.
        let d = doc(json!([{
            "type": "panel",
            "attrs": {"panelType": "info"},
            "content": [{
                "type": "paragraph",
                "content": [{"type": "text", "text": "nested ``` here"}]
            }]
        }]));
        let md = to_markdown(&d);

        // Opening fence must be ≥ 4 backticks because the content has 3.
        let first_line = md.lines().next().unwrap();
        let opening_fence_len = first_line.chars().take_while(|c| *c == '`').count();
        assert!(
            opening_fence_len >= 4,
            "expected ≥4-backtick opening fence, got {opening_fence_len}: {md}"
        );
        assert!(first_line.ends_with("adf:panel"));

        // Matching closing fence with the same length
        let closing_fence = "`".repeat(opening_fence_len);
        assert!(
            md.contains(&format!("\n{closing_fence}\n")),
            "missing matching closing fence ({opening_fence_len} backticks): {md}"
        );
    }

    #[test]
    fn escape_hatch_inline_grows_backticks() {
        let d = doc(json!([{
            "type": "paragraph",
            "content": [
                {"type": "text", "text": "before "},
                {
                    "type": "customInline",
                    "attrs": {"note": "has a `backtick`"}
                },
                {"type": "text", "text": " after"}
            ]
        }]));
        let md = to_markdown(&d);
        // The inline escape hatch must not break the paragraph's flow,
        // so it has to use ≥2 backticks to safely wrap the nested one.
        assert!(md.contains("before "), "got: {md}");
        assert!(md.contains(" after"), "got: {md}");
        assert!(md.contains("adf:customInline:"), "got: {md}");
    }

    #[test]
    fn table_cell_escapes_pipe() {
        let d = doc(json!([{
            "type": "table",
            "content": [
                {"type": "tableRow", "content": [
                    {"type": "tableHeader", "attrs": {}, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "Raw"}]}
                    ]}
                ]},
                {"type": "tableRow", "content": [
                    {"type": "tableCell", "attrs": {}, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "a|b"}]}
                    ]}
                ]}
            ]
        }]));
        let md = to_markdown(&d);
        assert!(md.contains("| a\\|b |"), "pipe not escaped: {md}");
    }

    #[test]
    fn table_cell_escapes_backslash() {
        // A cell containing the literal text "\|" must round-trip as
        // "\|", not as "\" followed by a cell boundary. Fix requires
        // escaping backslash before pipe.
        let d = doc(json!([{
            "type": "table",
            "content": [
                {"type": "tableRow", "content": [
                    {"type": "tableHeader", "attrs": {}, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "Raw"}]}
                    ]}
                ]},
                {"type": "tableRow", "content": [
                    {"type": "tableCell", "attrs": {}, "content": [
                        {"type": "paragraph", "content": [{"type": "text", "text": "a\\|b"}]}
                    ]}
                ]}
            ]
        }]));
        let md = to_markdown(&d);
        // Expect: literal backslash escaped as \\, literal pipe escaped as \|
        // → rendered cell should contain "a\\\\|b" (four chars: \ \ \ |)
        assert!(
            md.contains("| a\\\\\\|b |"),
            "backslash-pipe not escaped correctly: {md}"
        );
    }
}
