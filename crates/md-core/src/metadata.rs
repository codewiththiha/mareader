//! What a Markdown file says about itself.
//!
//! A PDF carries a title and an author in its document info, which the engine
//! reads for it; a Markdown file's nearest equivalents are its first heading
//! and, for files written for a static site or a book tool, a leading
//! front-matter block. Both are read here so the open flow asks one question
//! ([`document_title`]) instead of learning two formats' conventions.
//!
//! The front-matter reader is a SCALAR reader, not a YAML parser: it takes the
//! top-level `key: value` lines of the block and nothing else. That is the
//! whole contract a reading view needs — a title and an author are one line
//! each — and it is why this crate still has no Markdown or YAML dependency.
//! Anything the front matter nests (an `authors:` list, a `date:` map) is
//! skipped rather than guessed at.

use reflow_core::source::normalize;

use crate::ast::heading_of_line;

/// The front matter of a normalized source: the text between a leading `---`
/// line and its closer (`---` or `...`), without either marker.
///
/// A block that never closes is not front matter. An empty one is — a file that
/// opens with `---` / `---` is saying "the convention applies, there is nothing
/// to read", and the body after it must still be the body.
pub fn front_matter(normalized: &str) -> Option<String> {
    let mut after_opener = normalized.strip_prefix("---")?.strip_prefix('\n')?;
    let mut body: Vec<&str> = Vec::new();
    loop {
        let Some(line) = next_line(&mut after_opener) else {
            return None; // no closer: prose that happened to start with `---`
        };
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            return Some(body.join("\n"));
        }
        body.push(line);
    }
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

/// The value of a top-level front-matter key: quotes and a trailing comment
/// stripped, indentation honoured. `None` when the key is absent, empty, or
/// nested inside something else.
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

/// The document's title: a front-matter `title` when the file has one, else the
/// first ATX heading of levels 1–3 (deeper headings are sectioning, not a
/// title). `None` when the file claims neither, which is what lets the file
/// stem stand in.
pub fn document_title(raw: &str) -> Option<String> {
    let text = normalize(raw);
    if let Some(matter) = front_matter(&text)
        && let Some(title) = front_matter_value(&matter, "title")
    {
        return Some(title);
    }
    // The fallback reads the body: a front-matter block that carries no title
    // must not consume the heading under it, and its `---` line is not a
    // heading, so scanning from the top of the file would find nothing.
    first_heading_title(after_front_matter(&text))
}

/// The file with a closed front-matter block removed. A file without one is
/// returned as it came; the markers only belong to front matter when they close.
fn after_front_matter(normalized: &str) -> &str {
    let mut rest = match normalized
        .strip_prefix("---")
        .and_then(|r| r.strip_prefix('\n'))
    {
        Some(rest) => rest,
        None => return normalized,
    };
    while let Some(line) = next_line(&mut rest) {
        let trimmed = line.trim();
        if trimmed == "---" || trimmed == "..." {
            return rest;
        }
    }
    normalized
}

/// The document's author, from front matter only. Markdown has no
/// heading-level convention for it, and inventing one ("the second paragraph is
/// the author") would be wrong for the many files that have no author at all.
pub fn document_author(raw: &str) -> Option<String> {
    let text = normalize(raw);
    front_matter(&text).and_then(|matter| front_matter_value(&matter, "author"))
}

/// The first heading's text, skipping fences so a `#` inside a code sample is
/// not mistaken for the document's name.
fn first_heading_title(normalized: &str) -> Option<String> {
    let mut in_fence = false;
    for line in normalized.split('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.is_empty() {
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
        assert_eq!(first_heading_title("# Dune\n\nby Frank Herbert").as_deref(), Some("Dune"));
        assert_eq!(first_heading_title("## Part One").as_deref(), Some("Part One"));
        assert_eq!(first_heading_title("#### too deep"), None);
        assert_eq!(first_heading_title("plain prose first\n\n# later"), None);
        assert_eq!(first_heading_title("#"), None);
        assert_eq!(first_heading_title(""), None);
        // A shell prompt inside a sample is not the document's name.
        assert_eq!(first_heading_title("```\n# make install\n```\n\n# Build notes"), Some("Build notes".into()));
    }

    #[test]
    fn front_matter_needs_to_open_and_close_the_file() {
        assert_eq!(front_matter("---\ntitle: Dune\n---\n\nBody").as_deref(), Some("title: Dune"));
        assert_eq!(front_matter("---\ntitle: Dune\n...\nBody").as_deref(), Some("title: Dune"));
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
        assert_eq!(front_matter_value(matter, "author").as_deref(), Some("Frank Herbert"));
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
        assert_eq!(document_title("# Right Heading\n\nbody").as_deref(), Some("Right Heading"));
        // A front-matter block with no title does NOT consume the heading.
        assert_eq!(
            document_title("---\nlang: en\n---\n\n# Dune").as_deref(),
            Some("Dune")
        );
        assert_eq!(document_title("just prose"), None);
    }

    #[test]
    fn author_comes_from_front_matter_only() {
        assert_eq!(document_author("---\nauthor: 'F. Herbert'\n---").as_deref(), Some("F. Herbert"));
        assert_eq!(document_author("# Dune\n\nby Frank Herbert"), None);
    }
}
