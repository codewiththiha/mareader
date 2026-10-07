//! What a Markdown file says about itself: front matter and its first heading.

use reflow_core::block::FenceTracker;
use reflow_core::source::normalize;

use crate::ast::heading_of_line;

/// The front matter of a normalized source, without either marker.
fn front_matter(normalized: &str) -> Option<String> {
    split_front_matter(normalized).map(|(matter, _)| matter.to_string())
}

/// One `key: value` line at a time, from the remainder of the source.
fn next_line<'a>(rest: &mut &'a str) -> Option<&'a str> {
    if rest.is_empty() {
        return None;
    }
    let (line, tail) = match rest.find('\n') {
        Some(at) => (&rest[..at], &rest[at + 1..]),
        None => (*rest, ""),
    };
    *rest = tail;
    Some(line)
}

/// The leading front-matter block and the body after it, in one scan.
fn split_front_matter(normalized: &str) -> Option<(&str, &str)> {
    let mut rest = normalized.strip_prefix("---")?.strip_prefix('\n')?;
    // Byte offset just past the line `next_line` consumed last.
    let mut consumed = "---\n".len();
    loop {
        let before = consumed;
        let Some(line) = next_line(&mut rest) else {
            return None; // no closer: prose that happened to start with `---`
        };
        consumed += line.len() + 1;
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            // Matter ends before the closer; a bare closer is empty.
            let matter_end = if before > "---\n".len() {
                before - 1
            } else {
                before
            };
            return Some((&normalized["---\n".len()..matter_end], rest));
        }
    }
}

/// The value of a top-level front-matter key, quotes and comments stripped.
fn front_matter_value(matter: &str, key: &str) -> Option<String> {
    for line in matter.split('\n') {
        // Leading whitespace means the key belongs to a nested structure.
        if line.starts_with([' ', '\t']) {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.trim() != key {
            continue;
        }
        // `title: Dune # a draft` — the comment is not part of the value.
        let value = value.split('#').next().unwrap_or("").trim();
        let value = strip_quotes(value);
        return (!value.is_empty()).then(|| value.to_string());
    }
    None
}

/// The title: a front-matter `title`, else the first ATX heading of levels 1-3.
pub fn document_title(raw: &str) -> Option<String> {
    let text = normalize(raw);
    let (matter, body) = match split_front_matter(&text) {
        Some((matter, after)) => (matter, after),
        None => ("", text.as_str()),
    };
    if let Some(title) = front_matter_value(matter, "title") {
        return Some(title);
    }
    // The fallback reads the body, past a title-less front matter.
    first_heading_title(body)
}

/// The author, from front matter only: Markdown has no heading convention.
pub fn document_author(raw: &str) -> Option<String> {
    let text = normalize(raw);
    front_matter(&text).and_then(|matter| front_matter_value(&matter, "author"))
}

/// The first heading's text, skipping fences via the shared FenceTracker.
fn first_heading_title(normalized: &str) -> Option<String> {
    let mut fences = FenceTracker::default();
    for line in normalized.split('\n') {
        let trimmed = line.trim();
        if fences.feed(trimmed) {
            continue;
        }
        if fences.inside() || trimmed.is_empty() {
            continue;
        }
        let (level, title) = heading_of_line(trimmed)?;
        return (1..=3).contains(&level).then_some(title);
    }
    None
}

/// `"Dune"`, `'Dune'` and `Dune` all mean Dune; anything else is the value.
fn strip_quotes(value: &str) -> &str {
    for quote in ['"', '\''] {
        if value.len() >= 2 && value.starts_with(quote) && value.ends_with(quote) {
            return value[1..value.len() - 1].trim();
        }
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_title_is_the_first_shallow_heading() {
        assert_eq!(
            first_heading_title("# Dune\n\nby Frank Herbert").as_deref(),
            Some("Dune")
        );
        assert_eq!(
            first_heading_title("## Part One").as_deref(),
            Some("Part One")
        );
        assert_eq!(first_heading_title("#### too deep"), None);
        assert_eq!(first_heading_title("plain prose first\n\n# later"), None);
        assert_eq!(first_heading_title("#"), None);
        assert_eq!(first_heading_title(""), None);
        // A shell prompt inside a sample is not the document's name.
        assert_eq!(
            first_heading_title("```\n# make install\n```\n\n# Build notes"),
            Some("Build notes".into())
        );
        // An info-string line inside a fence is content, not a close.
        assert_eq!(
            first_heading_title("```\nsample code\n~~~rs\n# an inner prompt\n```\n\n# Real Title"),
            Some("Real Title".into())
        );
    }

    #[test]
    fn front_matter_needs_to_open_and_close_the_file() {
        assert_eq!(
            front_matter("---\ntitle: Dune\n---\n\nBody").as_deref(),
            Some("title: Dune")
        );
        assert_eq!(
            front_matter("---\ntitle: Dune\n...\nBody").as_deref(),
            Some("title: Dune")
        );
        assert_eq!(front_matter("---\n---\nBody").as_deref(), Some(""));
        // Never closed: not front matter, so the block stays in the body.
        assert_eq!(front_matter("---\ntitle: Dune\n\nBody"), None);
        // Not alone on its line: a thematic break, not a delimiter.
        assert_eq!(front_matter("--- and more"), None);
        assert_eq!(front_matter("# Title\n\n---\ntitle: nope\n---"), None);
    }

    #[test]
    fn scalar_values_are_read_and_noise_stripped() {
        let matter = "title: \"Dune\"\nauthor: Frank Herbert # the novel\nnested:\n  title: wrong\nlist:\n  - one";
        assert_eq!(front_matter_value(matter, "title").as_deref(), Some("Dune"));
        assert_eq!(
            front_matter_value(matter, "author").as_deref(),
            Some("Frank Herbert")
        );
        // The nested key is not a top-level one.
        assert_eq!(front_matter_value(matter, "wrong"), None);
        assert_eq!(front_matter_value(matter, "list"), None);
        assert_eq!(front_matter_value(matter, "missing"), None);
        let empty = "title:\nauthor: \"\"";
        assert_eq!(front_matter_value(empty, "title"), None);
        assert_eq!(front_matter_value(empty, "author"), None);
    }

    #[test]
    fn front_matter_wins_over_the_heading_and_the_heading_is_the_fallback() {
        assert_eq!(
            document_title("---\ntitle: Dune\n---\n\n# Wrong Heading").as_deref(),
            Some("Dune")
        );
        assert_eq!(
            document_title("# Right Heading\n\nbody").as_deref(),
            Some("Right Heading")
        );
        // A front-matter block with no title does NOT consume the heading.
        assert_eq!(
            document_title("---\nlang: en\n---\n\n# Dune").as_deref(),
            Some("Dune")
        );
        assert_eq!(document_title("just prose"), None);
    }

    #[test]
    fn author_comes_from_front_matter_only() {
        assert_eq!(
            document_author("---\nauthor: 'F. Herbert'\n---").as_deref(),
            Some("F. Herbert")
        );
        assert_eq!(document_author("# Dune\n\nby Frank Herbert"), None);
    }
}
