//! Markdown -> ADF.
//!
//! Event-driven walker over pulldown-cmark. A small frame stack tracks
//! open block containers (doc, paragraph, heading, lists, blockquote,
//! code block), and a parallel marks stack tracks inline decorations
//! (strong, em, code, strike, link).
//!
//! Fenced code blocks whose language starts with `adf:` are the escape
//! hatch — their body is parsed as JSON and emitted as a raw ADF node,
//! which is how exotic nodes like panels and status lozenges round-trip
//! losslessly through `from_adf -> to_adf`.

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use serde_json::{Value, json};

use crate::AdfDocument;

pub fn to_adf(md: &str) -> AdfDocument {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(md, opts);

    let mut stack: Vec<Frame> = vec![Frame::Doc(Vec::new())];
    let mut marks: Vec<Value> = Vec::new();

    for event in parser {
        match event {
            Event::Start(tag) => handle_start(&mut stack, &mut marks, tag),
            Event::End(end) => handle_end(&mut stack, &mut marks, end),
            Event::Text(text) => push_inline(&mut stack, text_node(&text, &marks)),
            Event::Code(text) => {
                let mut merged = marks.clone();
                merged.push(json!({"type": "code"}));
                push_inline(&mut stack, text_node(&text, &merged));
            }
            Event::SoftBreak => push_inline(&mut stack, text_node(" ", &marks)),
            Event::HardBreak => push_inline(&mut stack, json!({"type": "hardBreak"})),
            Event::Rule => push_block(&mut stack, json!({"type": "rule"})),
            _ => {} // footnotes, html, tasklists, metadata: ignored for now
        }
    }

    // Flush any still-open frames (defensive).
    while stack.len() > 1 {
        let node = close_frame(&mut stack);
        push_block(&mut stack, node);
    }

    match stack.pop() {
        Some(Frame::Doc(content)) => json!({
            "type": "doc",
            "version": 1,
            "content": content,
        }),
        _ => json!({"type": "doc", "version": 1, "content": []}),
    }
}

enum Frame {
    Doc(Vec<Value>),
    Paragraph(Vec<Value>),
    Heading(u32, Vec<Value>),
    BulletList(Vec<Value>),
    OrderedList(Vec<Value>),
    Item(Vec<Value>),
    Blockquote(Vec<Value>),
    CodeBlock { lang: String, text: String },
}

fn handle_start(stack: &mut Vec<Frame>, marks: &mut Vec<Value>, tag: Tag) {
    match tag {
        Tag::Paragraph => stack.push(Frame::Paragraph(Vec::new())),
        Tag::Heading { level, .. } => {
            let lv = match level {
                HeadingLevel::H1 => 1,
                HeadingLevel::H2 => 2,
                HeadingLevel::H3 => 3,
                HeadingLevel::H4 => 4,
                HeadingLevel::H5 => 5,
                HeadingLevel::H6 => 6,
            };
            stack.push(Frame::Heading(lv, Vec::new()));
        }
        Tag::BlockQuote(_) => stack.push(Frame::Blockquote(Vec::new())),
        Tag::CodeBlock(kind) => {
            let lang = match kind {
                CodeBlockKind::Fenced(s) => s.to_string(),
                CodeBlockKind::Indented => String::new(),
            };
            stack.push(Frame::CodeBlock { lang, text: String::new() });
        }
        Tag::List(Some(_)) => stack.push(Frame::OrderedList(Vec::new())),
        Tag::List(None) => stack.push(Frame::BulletList(Vec::new())),
        Tag::Item => stack.push(Frame::Item(Vec::new())),
        Tag::Emphasis => marks.push(json!({"type": "em"})),
        Tag::Strong => marks.push(json!({"type": "strong"})),
        Tag::Strikethrough => marks.push(json!({"type": "strike"})),
        Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
            // Images map to links for now — until the attachment upload flow
            // is wired up, turning `![alt](./path)` into a proper mediaSingle
            // would emit an unresolvable reference.
            marks.push(json!({
                "type": "link",
                "attrs": {"href": dest_url.to_string()}
            }));
        }
        _ => {}
    }
}

fn handle_end(stack: &mut Vec<Frame>, marks: &mut Vec<Value>, end: TagEnd) {
    match end {
        TagEnd::Emphasis
        | TagEnd::Strong
        | TagEnd::Strikethrough
        | TagEnd::Link
        | TagEnd::Image => {
            marks.pop();
        }
        TagEnd::Item => {
            // Tight list items skip the Paragraph wrapper; we may have an
            // implicit paragraph still open from push_inline. Close it first.
            if matches!(stack.last(), Some(Frame::Paragraph(_))) {
                let node = close_frame(stack);
                push_block(stack, node);
            }
            if stack.len() > 1 {
                let node = close_frame(stack);
                push_block(stack, node);
            }
        }
        TagEnd::Paragraph
        | TagEnd::Heading(_)
        | TagEnd::BlockQuote(_)
        | TagEnd::CodeBlock
        | TagEnd::List(_) => {
            if stack.len() > 1 {
                let node = close_frame(stack);
                push_block(stack, node);
            }
        }
        _ => {}
    }
}

fn close_frame(stack: &mut Vec<Frame>) -> Value {
    let frame = stack.pop().expect("non-empty stack");
    match frame {
        Frame::Doc(content) => json!({"type": "doc", "version": 1, "content": content}),
        Frame::Paragraph(content) => json!({"type": "paragraph", "content": content}),
        Frame::Heading(level, content) => json!({
            "type": "heading",
            "attrs": {"level": level},
            "content": content,
        }),
        Frame::BulletList(items) => json!({"type": "bulletList", "content": items}),
        Frame::OrderedList(items) => json!({"type": "orderedList", "content": items}),
        Frame::Item(content) => json!({"type": "listItem", "content": content}),
        Frame::Blockquote(content) => json!({"type": "blockquote", "content": content}),
        Frame::CodeBlock { lang, text } => {
            if lang.starts_with("adf:")
                && let Ok(raw) = serde_json::from_str::<Value>(text.trim())
            {
                return raw;
            }
            let mut attrs = serde_json::Map::new();
            if !lang.is_empty() {
                attrs.insert("language".into(), json!(lang));
            }
            let content_text = text.trim_end_matches('\n').to_string();
            json!({
                "type": "codeBlock",
                "attrs": attrs,
                "content": [{"type": "text", "text": content_text}]
            })
        }
    }
}

fn push_block(stack: &mut Vec<Frame>, node: Value) {
    if let Some(top) = stack.last_mut() {
        let container = match top {
            Frame::Doc(v)
            | Frame::BulletList(v)
            | Frame::OrderedList(v)
            | Frame::Item(v)
            | Frame::Blockquote(v) => v,
            _ => return,
        };
        container.push(node);
    }
}

fn push_inline(stack: &mut Vec<Frame>, node: Value) {
    if let Some(Frame::CodeBlock { text, .. }) = stack.last_mut() {
        if let Some(t) = node.get("text").and_then(Value::as_str) {
            text.push_str(t);
        }
        return;
    }
    match stack.last_mut() {
        Some(Frame::Paragraph(v)) | Some(Frame::Heading(_, v)) => {
            v.push(node);
        }
        // Tight list items land here: pulldown-cmark emits Text directly
        // inside Item without a surrounding Paragraph. Open an implicit
        // paragraph so ADF stays well-formed (listItem requires a block
        // child). TagEnd::Item closes this before closing the item itself.
        Some(Frame::Item(_)) => {
            stack.push(Frame::Paragraph(vec![node]));
        }
        _ => {}
    }
}

fn text_node(text: &str, marks: &[Value]) -> Value {
    if marks.is_empty() {
        json!({"type": "text", "text": text})
    } else {
        json!({"type": "text", "text": text, "marks": marks})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::from_adf::to_markdown;

    fn roundtrip(md: &str) -> String {
        to_markdown(&to_adf(md))
    }

    #[test]
    fn empty() {
        let adf = to_adf("");
        assert_eq!(adf["type"], "doc");
        assert_eq!(adf["content"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn simple_paragraph() {
        let adf = to_adf("hello world\n");
        assert_eq!(
            adf,
            json!({
                "type": "doc",
                "version": 1,
                "content": [{
                    "type": "paragraph",
                    "content": [{"type": "text", "text": "hello world"}]
                }]
            })
        );
    }

    #[test]
    fn roundtrip_paragraph() {
        assert_eq!(roundtrip("hello world\n"), "hello world\n");
    }

    #[test]
    fn roundtrip_heading() {
        assert_eq!(roundtrip("# Top\n"), "# Top\n");
        assert_eq!(roundtrip("### Sub\n"), "### Sub\n");
    }

    #[test]
    fn roundtrip_marks() {
        assert_eq!(
            roundtrip("**bold** *italic* `code`\n"),
            "**bold** *italic* `code`\n"
        );
    }

    #[test]
    fn roundtrip_strikethrough() {
        assert_eq!(roundtrip("~~gone~~\n"), "~~gone~~\n");
    }

    #[test]
    fn roundtrip_link() {
        assert_eq!(
            roundtrip("[click](https://example.com)\n"),
            "[click](https://example.com)\n"
        );
    }

    #[test]
    fn roundtrip_code_block_with_lang() {
        assert_eq!(
            roundtrip("```rust\nfn main() {}\n```\n"),
            "```rust\nfn main() {}\n```\n"
        );
    }

    #[test]
    fn roundtrip_bullet_list() {
        assert_eq!(roundtrip("- one\n- two\n"), "- one\n- two\n");
    }

    #[test]
    fn roundtrip_ordered_list() {
        assert_eq!(roundtrip("1. first\n2. second\n"), "1. first\n2. second\n");
    }

    #[test]
    fn roundtrip_blockquote() {
        assert_eq!(roundtrip("> quoted\n"), "> quoted\n");
    }

    #[test]
    fn adf_escape_hatch_preserves_raw_node() {
        let md = "```adf:panel\n\
                  {\"type\":\"panel\",\"attrs\":{\"panelType\":\"info\"},\"content\":[]}\n\
                  ```\n";
        let adf = to_adf(md);
        let content = adf["content"].as_array().unwrap();
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "panel");
        assert_eq!(content[0]["attrs"]["panelType"], "info");
    }

    #[test]
    fn invalid_adf_escape_hatch_falls_back_to_code_block() {
        let md = "```adf:panel\nnot valid json\n```\n";
        let adf = to_adf(md);
        assert_eq!(adf["content"][0]["type"], "codeBlock");
    }
}
