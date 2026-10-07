//! Display-name derivation for the open document: a trustworthy `/Title`,
//! else the file name.

const MAX_TITLE_LEN: usize = 200;

/// The display name: a trustworthy `/Title`, else the file name.
pub fn display_name(title: Option<&str>, path: Option<&str>) -> Option<String> {
    if let Some(t) = document_title(title) {
        return Some(t);
    }
    path.and_then(file_stem_from_path).filter(|s| !s.is_empty())
}

/// The document's own title, when it is worth showing.
pub fn document_title(title: Option<&str>) -> Option<String> {
    title
        .map(str::trim)
        .filter(|t| is_usable_title(t))
        .map(str::to_string)
}

/// True when a title beats the file name.
pub fn is_usable_title(t: &str) -> bool {
    if t.is_empty() || t.chars().count() > MAX_TITLE_LEN {
        return false;
    }
    let lower = t.to_lowercase();
    const PLACEHOLDERS: [&str; 5] = [
        "untitled",
        "unknown",
        "document",
        "no title",
        "pdf document",
    ];
    if PLACEHOLDERS.contains(&lower.as_str()) {
        return false;
    }
    if t.contains("://") || t.contains('\\') || t.contains('%') {
        return false;
    }
    if looks_like_file_name(t) {
        return false;
    }
    t.chars().any(|c| c.is_alphanumeric())
}

/// The shapes a downloader leaves in `/Title`.
fn looks_like_file_name(t: &str) -> bool {
    // A title does not carry its own extension.
    if strip_doc_extension(t) != t {
        return true;
    }
    // ISBN/UPC digit runs of nine or more, separators aside.
    let run: String = t.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if run.len() >= 9
        && run
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')
        && run.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    // Snake-case means no spaces; a trailing `_N` counter is the exception.
    strip_copy_counter(t).contains('_') && !t.contains(' ')
}

/// Drop a trailing `_N` copy counter, when there is one.
pub fn strip_copy_counter(t: &str) -> &str {
    match t.rsplit_once('_') {
        Some((base, counter))
            if !base.is_empty()
                && !counter.is_empty()
                && counter.chars().all(|c| c.is_ascii_digit()) =>
        {
            base
        }
        _ => t,
    }
}

/// Human-readable file name for `path`, extension removed.
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

/// Extensions this app never admits but a name may still carry.
const OFFICE_AND_PRINT: [&str; 7] = ["doc", "docx", "ps", "dvi", "tex", "ppt", "pptx"];

/// Remove a trailing document extension, case-insensitively.
fn strip_doc_extension(s: &str) -> &str {
    let Some(dot) = s.rfind('.') else {
        return s;
    };
    if dot == 0 {
        return s;
    }
    let lower = s[dot + 1..].to_lowercase();
    let ext = lower.as_str();
    let known =
        crate::format::extensions().any(|kind| kind == ext) || OFFICE_AND_PRINT.contains(&ext);
    if known { &s[..dot] } else { s }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            display_name(Some("untitled"), Some("/x/y.pdf")).as_deref(),
            Some("y")
        );
        assert_eq!(display_name(None, None), None);
    }

    #[test]
    fn a_download_name_is_not_a_title() {
        // A downloader's /Title loses to the stem of the address.
        assert_eq!(
            display_name(Some("0321894073.pdf"), Some("/d/mathematical-proofs.pdf")).as_deref(),
            Some("mathematical-proofs")
        );
        assert_eq!(
            display_name(Some("032190026X"), Some("/d/graphical-approach.pdf")).as_deref(),
            Some("graphical-approach")
        );
        assert_eq!(
            display_name(
                Some("A_Graphical_Approach_to_Algebra_and_Trigonometry"),
                Some("/d/approach.pdf")
            )
            .as_deref(),
            Some("approach")
        );
        // Real titles stay titles; a mid-name dot is not an extension.
        assert_eq!(
            display_name(Some("1984"), Some("/d/1984.pdf")).as_deref(),
            Some("1984")
        );
        assert_eq!(
            display_name(Some("Mathematical Proofs"), Some("/d/mp.pdf")).as_deref(),
            Some("Mathematical Proofs")
        );
        assert_eq!(
            display_name(Some("Mr. Smith Goes West"), Some("/d/msgw.pdf")).as_deref(),
            Some("Mr. Smith Goes West")
        );
        assert!(!super::looks_like_file_name("Discrete Mathematics"));
        assert!(super::looks_like_file_name("978-0-321-89407-3"));
        // A trailing copy counter is a name someone actually chose.
        assert!(!super::looks_like_file_name("dune_1"));
        assert!(!super::looks_like_file_name("Dune_12"));
        // A counter on a mangled name does not launder the mangling.
        assert!(super::looks_like_file_name("harry_potter_goblet_1"));
    }

    #[test]
    fn the_document_title_is_the_metadata_half_alone() {
        // The half of `display_name` a persisted name is made of.
        assert_eq!(
            super::document_title(Some("  Dune  ")),
            Some("Dune".to_string())
        );
        assert_eq!(super::document_title(Some("dune.pdf")), None);
        assert_eq!(super::document_title(Some("")), None);
        assert_eq!(super::document_title(None), None);
    }

    #[test]
    fn file_stem_extraction() {
        for (path, want) in [
            (
                "/b/Programming Pearls (2nd Edition) - Jon Bentley.pdf",
                Some("Programming Pearls (2nd Edition) - Jon Bentley"),
            ),
            (r"C:\Users\me\Docs\Deep Work.pdf", Some("Deep Work")),
            (r"\\server\share\Annual Report.pdf", Some("Annual Report")),
            ("/a/b/", Some("b")),
            ("book.pdf", Some("book")),
            // The formats this app opens, not just pdf.js's.
            ("/books/notes.md", Some("notes")),
            ("/books/notes.markdown", Some("notes")),
            ("/books/notes.MDOWN", Some("notes")),
            ("/books/log.txt", Some("log")),
            ("/books/log.text", Some("log")),
            // The office and print kinds the gate refuses.
            ("/books/slides.pptx", Some("slides")),
            // A version is not an extension, nor is a hidden file's dot.
            ("/books/Rust 1.75", Some("Rust 1.75")),
            ("/books/.markdown", Some(".markdown")),
            ("/books/archive.epub", Some("archive.epub")),
            // Lowercasing can expand Unicode: measure the dot in the original.
            ("/books/İ.PDF", Some("İ")),
            ("/", None),
        ] {
            assert_eq!(file_stem_from_path(path).as_deref(), want, "path {path:?}");
        }
    }
}
