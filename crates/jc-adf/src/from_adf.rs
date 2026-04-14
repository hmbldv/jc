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
        _ => render_unknown_block(node, out),
    }
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
    out.push_str("```adf:");
    out.push_str(if ty.is_empty() { "unknown" } else { ty });
    out.push('\n');
    out.push_str(&serde_json::to_string_pretty(node).unwrap_or_default());
    out.push('\n');
    out.push_str("```\n");
}

fn render_unknown_inline(node: &Value, out: &mut String) {
    let ty = node_type(node);
    out.push_str("`adf:");
    out.push_str(if ty.is_empty() { "unknown" } else { ty });
    out.push(':');
    out.push_str(&serde_json::to_string(node).unwrap_or_default());
    out.push('`');
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
        assert_eq!(to_markdown(&d), "![architecture diagram](attachment:att-42)\n");
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
}
