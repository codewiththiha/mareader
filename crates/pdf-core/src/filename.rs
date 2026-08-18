//! Display-name derivation for the open document. Pure — no wasm deps.
//!
//! A PDF's `/Title` metadata is free-form and frequently garbage (producers
//! write source paths, percent-encoded URLs, or placeholders). Use it only
//! when it looks like a real title; otherwise fall back to the file name.

const MAX_TITLE_LEN: usize = 200;

/// The display name for the open document: a trustworthy `/Title`, else the
/// file name derived from `path`, else `None`.
pub fn display_name(title: Option<&str>, path: Option<&str>) -> Option<String> {
    if let Some(t) = title.map(str::trim).filter(|t| is_usable_title(t)) {
        return Some(t.to_string());
    }
    path.and_then(file_stem_from_path).filter(|s| !s.is_empty())
}

/// True when a title is worth showing instead of the file name: short enough,
/// not URL- or path-shaped, not a known placeholder, and not all punctuation.
fn is_usable_title(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > MAX_TITLE_LEN {
        return false;
    }
    let lower = t.to_lowercase();
    const PLACEHOLDERS: [&str; 5] = ["untitled", "unknown", "document", "no title", "pdf document"];
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return false;
    }
    if t.contains("://") || t.contains('\\') || t.contains('%') {
        return false;
    }
    t.chars().any(|c| c.is_alphanumeric())
}

/// Human-readable file name for `path`: last segment (splitting on both `/`
/// and `\\`), with a document extension removed.
pub fn file_stem_from_path(path: &str) -> Option<String> {
    let p = path.trim().trim_end_matches(['/', '\\']);
    if p.is_empty() {
        return None;
    }
    let last = p.rsplit(['/', '\\']).next().unwrap_or(p);
    let stem = strip_doc_extension(last.trim());
    if stem.is_empty() {
        None
    } else {
        Some(stem.to_string())
    }
}

/// Remove a trailing document extension (case-insensitive). A title like
/// "Rust 1.75" must keep its ".75".
fn strip_doc_extension(s: &str) -> &str {
    const EXTS: [&str; 8] = [".pdf", ".doc", ".docx", ".ps", ".dvi", ".tex", ".ppt", ".pptx"];
    let lower = s.to_lowercase();
    for ext in EXTS {
        if lower.ends_with(ext) && s.len() > ext.len() {
            return &s[..s.len() - ext.len()];
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Title-vs-path arbitration: a usable /Title wins, an unusable one falls
    /// back to the file name.
    #[test]
    fn picks_the_best_available_name() {
        assert_eq!(
            display_name(Some("The Rust Programming Language"), Some("/tmp/trpl.pdf")).as_deref(),
            Some("The Rust Programming Language")
        );
        // The canonical offender: Distiller wrote a truncated source path.
        assert_eq!(
            display_name(
                Some("file:///F|/Mis%20docum"),
                Some("/Users/me/Books/Programming Pearls (2nd Edition) - Jon Bentley.pdf"),
            )
            .as_deref(),
            Some("Programming Pearls (2nd Edition) - Jon Bentley")
        );
        // Placeholders and path-shaped titles fall back too.
        assert_eq!(display_name(Some("untitled"), Some("/x/y.pdf")).as_deref(), Some("y"));
        assert_eq!(display_name(None, None), None);
    }

    /// Extracting a display name from a path: separators (both kinds), and
    /// extension stripping.
    #[test]
    fn file_stem_extraction() {
        for (path, want) in [
            ("/b/Programming Pearls (2nd Edition) - Jon Bentley.pdf",
             Some("Programming Pearls (2nd Edition) - Jon Bentley")),
            (r"C:\Users\me\Docs\Deep Work.pdf", Some("Deep Work")),
            (r"\\server\share\Annual Report.pdf", Some("Annual Report")),
            ("/a/b/", Some("b")),
            ("book.pdf", Some("book")),
            ("/", None),
        ] {
            assert_eq!(file_stem_from_path(path).as_deref(), want, "path {path:?}");
        }
    }
}
