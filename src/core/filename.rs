//! Display-name derivation for the open document. Pure — no wasm deps, so the
//! whole rule set is unit-testable on the host.
//!
//! ## Why this exists
//!
//! The toolbar used to show `doc.title.or(doc.path)` verbatim. A PDF's `/Title`
//! metadata is free-form and, in practice, frequently garbage — it carries
//! whatever the producing tool happened to write decades ago. Real examples:
//!
//! ```text
//! "file:///F|/Mis%20docum"        <- dvips/Acrobat Distiller wrote the source
//!                                    path (truncated, percent-encoded, with the
//!                                    old Windows "F|" drive spelling)
//! "Microsoft Word - Chapter3.doc" <- Word's PDF export
//! "untitled"                      <- LaTeX / Preview defaults
//! ```
//!
//! That is why "Programming Pearls (2nd Edition) - Jon Bentley.pdf" displayed as
//! `file:///F|/Mis%20docum` while other files looked fine: those other files
//! simply had a sane `/Title`, or none at all (so the path fallback ran).
//!
//! The rule here: **use `/Title` only when it actually looks like a title.**
//! Anything path-shaped, URL-shaped, percent-encoded, filename-with-extension,
//! or a known placeholder is rejected, and the human-readable stem of the real
//! file path is used instead — which is what the user recognises anyway.
//!
//! The path fallback itself also has to be right: split on BOTH separators
//! (a Windows path reaching a mac/Linux build must not be treated as one long
//! filename), percent-decode, and drop the `.pdf` extension.

/// Longest `/Title` we will trust. Metadata titles are book/paper titles;
/// anything past this is a pasted path or an abstract, not a name for a 40-char
/// toolbar slot.
const MAX_TITLE_LEN: usize = 200;

/// The display name for the open document: a trustworthy `//Title`, else the
/// file name derived from `path`, else `None`.
///
/// `title` is the raw PDF metadata title, `path` the file path (or URL) the
/// document was opened from.
pub fn display_name(title: Option<&str>, path: Option<&str>) -> Option<String> {
    if let Some(t) = title.map(clean_title).filter(|t| is_usable_title(t)) {
        return Some(t);
    }
    path.and_then(|p| file_stem_from_path(p)).filter(|s| !s.is_empty())
}

/// Normalise a raw metadata title: strip BOM/zero-width junk, collapse runs of
/// whitespace (some producers embed newlines), and trim.
fn clean_title(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut last_space = false;
    for ch in raw.chars() {
        // U+FEFF (BOM) and U+200B..U+200D (zero-width) survive round-trips
        // through many producers and would otherwise defeat the trims below.
        if ch == '\u{feff}' || ('\u{200b}'..='\u{200d}').contains(&ch) {
            continue;
        }
        if ch.is_whitespace() || ch.is_control() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(ch);
            last_space = false;
        }
    }
    // Word's export prefix is pure noise: "Microsoft Word - Report.doc".
    let trimmed = out.trim();
    let stripped = trimmed
        .strip_prefix("Microsoft Word - ")
        .or_else(|| trimmed.strip_prefix("Microsoft PowerPoint - "))
        .or_else(|| trimmed.strip_prefix("Microsoft Excel - "))
        .unwrap_or(trimmed);
    strip_doc_extension(stripped).trim().to_string()
}

/// True when a cleaned title is worth showing instead of the file name.
///
/// Rejects (in order): empty/too-long, known placeholders, URLs, anything
/// path-shaped, percent-encoded blobs, and strings with no letters or digits.
fn is_usable_title(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > MAX_TITLE_LEN {
        return false;
    }

    let lower = t.to_lowercase();

    // Producer placeholders.
    const PLACEHOLDERS: [&str; 8] = [
        "untitled",
        "untitled document",
        "unknown",
        "document",
        "document1",
        "no title",
        "(untitled)",
        "pdf document",
    ];
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return false;
    }

    // URLs / URIs — "file:///F|/Mis%20docum" is the canonical offender.
    if lower.starts_with("file:")
        || lower.starts_with("http:")
        || lower.starts_with("https:")
        || lower.contains("://")
    {
        return false;
    }

    // Path-shaped: a Windows drive prefix ("C:\", and the archaic "F|/" that
    // old dvips/Distiller URL-escaping produced), a UNC/absolute root, or a
    // home-relative path.
    if looks_like_windows_drive(t)
        || t.starts_with('/')
        || t.starts_with("\\\\")
        || t.starts_with("~/")
        || t.starts_with("./")
        || t.starts_with("../")
    {
        return false;
    }

    // Embedded separators: a real title may contain a slash ("Either/Or"), but
    // a backslash or a percent-encoded byte is a path artifact, and so is more
    // than one slash.
    if t.contains('\\') || has_percent_escape(t) || t.matches('/').count() > 1 {
        return false;
    }

    // At least one alphanumeric character — reject "___" / "- - -" junk.
    if !t.chars().any(|c| c.is_alphanumeric()) {
        return false;
    }

    true
}

/// True for `C:\...`, `C:/...` and the legacy escaped form `C|/...`.
fn looks_like_windows_drive(s: &str) -> bool {
    let b: Vec<char> = s.chars().take(3).collect();
    b.len() >= 3
        && b[0].is_ascii_alphabetic()
        && (b[1] == ':' || b[1] == '|')
        && (b[2] == '\\' || b[2] == '/')
}

/// True when `s` contains a `%XX` escape (percent-encoded path/URL).
fn has_percent_escape(s: &str) -> bool {
    let b = s.as_bytes();
    b.windows(3).any(|w| {
        w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit()
    })
}

/// Human-readable file name for `path`: last segment (splitting on BOTH `/` and
/// `\`), percent-decoded, with a document extension removed.
///
/// Handles plain filesystem paths, Windows paths, and `file://` URLs (including
/// query/fragment suffixes that would otherwise end up in the name).
pub fn file_stem_from_path(path: &str) -> Option<String> {
    let mut p = path.trim();
    if p.is_empty() {
        return None;
    }

    // Drop a URL scheme and any query/fragment before splitting.
    if let Some(rest) = p.split_once("://").map(|(_, r)| r) {
        p = rest;
    }
    p = p.split(['?', '#']).next().unwrap_or(p);
    // Trailing separators from a directory-ish path.
    let p = p.trim_end_matches(['/', '\\']);

    let last = p.rsplit(['/', '\\']).next().unwrap_or(p);
    let decoded = percent_decode(last);
    // The archaic "F|" drive spelling can survive as the whole segment.
    let decoded = decoded.trim_start_matches(|c: char| c == '|').to_string();
    let stem = strip_doc_extension(decoded.trim()).trim().to_string();
    if stem.is_empty() {
        None
    } else {
        Some(stem)
    }
}

/// Remove a trailing document extension (case-insensitive). Only document
/// extensions — a title like "Rust 1.75" must keep its ".75".
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

/// Decode `%XX` escapes as UTF-8. Invalid escapes are left verbatim (a literal
/// `%` in a real filename must survive), and `+` is NOT treated as a space —
/// this decodes paths, not form encodings.
fn percent_decode(s: &str) -> String {
    if !s.contains('%') {
        return s.to_string();
    }
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Malformed sequences must not lose the rest of the name.
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Title-vs-path arbitration: a usable /Title wins, an unusable one falls
    /// back to the file name, and with neither there is nothing to show.
    #[test]
    fn picks_the_best_available_name() {
        // A good title wins over the path.
        assert_eq!(
            display_name(Some("The Rust Programming Language"), Some("/tmp/trpl.pdf")).as_deref(),
            Some("The Rust Programming Language")
        );
        // The exact bug: Distiller wrote a truncated source path as /Title, so
        // the file name has to take over.
        assert_eq!(
            display_name(
                Some("file:///F|/Mis%20docum"),
                Some("/Users/me/Books/Programming Pearls (2nd Edition) - Jon Bentley.pdf"),
            )
            .as_deref(),
            Some("Programming Pearls (2nd Edition) - Jon Bentley")
        );
        // Office exports and stray extensions are stripped off the title.
        assert_eq!(
            display_name(Some("Microsoft Word - Chapter3.doc"), Some("/x/y.pdf")).as_deref(),
            Some("Chapter3")
        );
        assert_eq!(
            display_name(Some("Thesis Draft.pdf"), Some("/x/y.pdf")).as_deref(),
            Some("Thesis Draft")
        );
        // Embedded runs of whitespace collapse to single spaces.
        assert_eq!(
            display_name(Some("A  Tale\nof\tTwo   Cities"), None).as_deref(),
            Some("A Tale of Two Cities")
        );
        // Nothing usable anywhere.
        assert_eq!(display_name(None, None), None);
        assert_eq!(display_name(Some("  "), None), None);
        assert_eq!(display_name(Some("untitled"), Some("")), None);
    }

    /// What counts as a usable title. Placeholders, URLs and path-shaped
    /// strings are rejected; ordinary prose — including slashes, colons and
    /// version numbers — is kept.
    #[test]
    fn title_usability() {
        for junk in [
            "untitled", "Untitled", "UNKNOWN",
            "file:///F|/Mis%20docum", "http://example.com/a.pdf",
            r"C:\Users\me\thesis.tex", "/home/me/thesis.pdf", "~/notes.pdf",
            "a/b/c/d", "Mis%20documentos", "___", "", "   ",
        ] {
            assert!(!is_usable_title(&clean_title(junk)), "should have rejected {junk:?}");
        }
        for good in [
            "Programming Pearls",
            "Either/Or: A Fragment of Life",
            "Chapter 7 — Concurrency",
            "Rust 1.75 Release Notes",
            "C. Elegans: a study",
        ] {
            assert!(is_usable_title(&clean_title(good)), "should have accepted {good:?}");
        }
        // Length cap, exactly at the boundary.
        assert!(is_usable_title(&"x".repeat(MAX_TITLE_LEN)));
        assert!(!is_usable_title(&"x".repeat(MAX_TITLE_LEN + 1)));
    }

    /// Extracting a display name from a path: separators (both kinds), percent
    /// escapes, extensions and the degenerate shapes.
    #[test]
    fn file_stem_extraction() {
        for (path, want) in [
            // Unicode, punctuation and spacing all survive intact.
            ("/b/Programming Pearls (2nd Edition) - Jon Bentley.pdf",
             Some("Programming Pearls (2nd Edition) - Jon Bentley")),
            ("/b/Kernighan & Ritchie — C, 2nd ed. [ANSI].pdf",
             Some("Kernighan & Ritchie — C, 2nd ed. [ANSI]")),
            ("/b/日本語のファイル.pdf", Some("日本語のファイル")),
            ("/b/report_v1.2_final.pdf", Some("report_v1.2_final")),
            // Percent escapes, including multi-byte UTF-8.
            ("file:///C:/Docs/My%20Book%20(1).pdf", Some("My Book (1)")),
            ("/x/Caf%C3%A9%20Notes.pdf", Some("Café Notes")),
            // A literal percent that is not an escape survives.
            ("/x/100%25 done.pdf", Some("100% done")),
            ("/x/50% off.pdf", Some("50% off")),
            // Windows and UNC separators.
            (r"C:\Users\me\Docs\Deep Work.pdf", Some("Deep Work")),
            (r"\\server\share\Annual Report.pdf", Some("Annual Report")),
            // Trailing separator, bare name, and nothing at all.
            ("/a/b/", Some("b")),
            ("book.pdf", Some("book")),
            ("/", None),
        ] {
            assert_eq!(file_stem_from_path(path).as_deref(), want, "path {path:?}");
        }
    }
}
