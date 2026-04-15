//! Markdown image pre-processor.
//!
//! Walks a markdown body, collects references to local image files
//! (anything that isn't already a URL scheme), and lets the caller
//! upload them to the correct Atlassian target before converting to
//! ADF. This closes the "markdown native" promise for rich content:
//! users write `![diagram](./arch.png)` and it becomes a real
//! attachment on the target issue/page instead of a broken link.
//!
//! The API is deliberately two-step — [`find_local_images`] is sync
//! and just enumerates the work to do, then the caller runs its async
//! upload loop, then [`rewrite_image_urls`] substitutes the resulting
//! IDs back into the markdown source. Splitting it this way avoids
//! fighting async closures through a generic and keeps each piece
//! trivially testable on its own.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use pulldown_cmark::{Event, Options, Parser, Tag};

#[derive(Debug, Clone)]
pub struct FoundImage {
    /// The URL as it appeared in the markdown source.
    pub original_url: String,
    /// Resolved filesystem path (relative URLs are joined against the
    /// markdown file's parent directory so images beside the file work
    /// regardless of the user's current working directory).
    pub resolved_path: PathBuf,
}

/// Find every local image reference in a markdown document.
///
/// Returns deduplicated results, one entry per unique URL, so the
/// caller uploads each file once even if it's referenced multiple
/// times. Relative URLs are resolved against `base_dir`; absolute
/// paths are passed through unchanged.
pub fn find_local_images(md: &str, base_dir: &Path) -> Vec<FoundImage> {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);

    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for event in Parser::new_ext(md, options) {
        if let Event::Start(Tag::Image { dest_url, .. }) = event {
            let url = dest_url.as_ref();
            if !is_local(url) {
                continue;
            }
            if !seen.insert(url.to_string()) {
                continue;
            }
            let resolved = if Path::new(url).is_absolute() {
                PathBuf::from(url)
            } else {
                base_dir.join(url)
            };
            out.push(FoundImage {
                original_url: url.to_string(),
                resolved_path: resolved,
            });
        }
    }
    out
}

/// Substitute image URLs in the markdown source. For each
/// `(old_url, new_url)` pair, occurrences of `(old_url)` in the source
/// become `(new_url)`. The leading `(` is part of the match so we only
/// touch URL positions inside link/image syntax, not bare filenames
/// that happen to appear in prose.
///
/// Multiple references to the same URL all get rewritten in one pass.
pub fn rewrite_image_urls(md: &str, replacements: &[(String, String)]) -> String {
    let mut out = md.to_string();
    for (old, new) in replacements {
        let needle = format!("({old})");
        let replacement = format!("({new})");
        out = out.replace(&needle, &replacement);
    }
    out
}

/// Classify a URL as "needs uploading" vs "already resolvable". Local
/// means no scheme and none of the reserved non-filesystem prefixes.
fn is_local(url: &str) -> bool {
    if url.is_empty() || url.contains("://") {
        return false;
    }
    for prefix in &["attachment:", "data:", "mailto:", "tel:", "#"] {
        if url.starts_with(prefix) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_single_image() {
        let md = "Here is a diagram: ![arch](./diagram.png)\n";
        let images = find_local_images(md, Path::new("/tmp/docs"));
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].original_url, "./diagram.png");
        assert_eq!(images[0].resolved_path, PathBuf::from("/tmp/docs/./diagram.png"));
    }

    #[test]
    fn skips_http_and_https() {
        let md = "![remote](https://example.com/img.png) ![also](http://example.com/x.png)\n";
        assert!(find_local_images(md, Path::new("/tmp")).is_empty());
    }

    #[test]
    fn skips_attachment_refs() {
        let md = "![existing](attachment:att-123)\n";
        assert!(find_local_images(md, Path::new("/tmp")).is_empty());
    }

    #[test]
    fn skips_data_urls() {
        let md = "![inline](data:image/png;base64,iVBOR...)\n";
        assert!(find_local_images(md, Path::new("/tmp")).is_empty());
    }

    #[test]
    fn dedupes_duplicate_urls() {
        let md = "![a](./img.png) and later ![b](./img.png)\n";
        let images = find_local_images(md, Path::new("/tmp"));
        assert_eq!(images.len(), 1);
    }

    #[test]
    fn finds_multiple_unique() {
        let md = "![a](./a.png) ![b](./b.png) ![c](./c.png)\n";
        let images = find_local_images(md, Path::new("/tmp"));
        assert_eq!(images.len(), 3);
    }

    #[test]
    fn absolute_paths_preserved() {
        let md = "![full](/var/img/pic.png)";
        let images = find_local_images(md, Path::new("/tmp"));
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].resolved_path, PathBuf::from("/var/img/pic.png"));
    }

    #[test]
    fn rewrite_substitutes_url() {
        let md = "See ![arch](./diagram.png)!\n";
        let out = rewrite_image_urls(
            md,
            &[("./diagram.png".to_string(), "attachment:att-123".to_string())],
        );
        assert_eq!(out, "See ![arch](attachment:att-123)!\n");
    }

    #[test]
    fn rewrite_handles_multiple() {
        let md = "![a](./a.png) and ![b](./b.png)";
        let out = rewrite_image_urls(
            md,
            &[
                ("./a.png".to_string(), "attachment:A".to_string()),
                ("./b.png".to_string(), "attachment:B".to_string()),
            ],
        );
        assert!(out.contains("(attachment:A)"));
        assert!(out.contains("(attachment:B)"));
        assert!(!out.contains("./a.png"));
        assert!(!out.contains("./b.png"));
    }

    #[test]
    fn rewrite_handles_duplicate_original() {
        // Both references to the same URL get rewritten in one pass.
        let md = "![first](./img.png) and ![second](./img.png)";
        let out = rewrite_image_urls(
            md,
            &[("./img.png".to_string(), "attachment:X".to_string())],
        );
        assert_eq!(out.matches("(attachment:X)").count(), 2);
        assert!(!out.contains("(./img.png)"));
    }
}
