//! Display-name derivation for the open document. Pure — no wasm deps.
//! A PDF's `/Title` is free-form and frequently garbage (source paths,
//! percent-encoded URLs, placeholders): use it only when it looks like a
//! real title, else fall back to the file name.

const MAX_TITLE_LEN: usize = 200;

/// The display name for the open document: a trustworthy `/Title`, else the
/// file name derived from `path`, else `None`.
pub fn display_name(title: Option<&str>, path: Option<&str>) -> Option<String> {
    if let Some(t) = document_title(title) {
        return Some(t);
    }
    path.and_then(file_stem_from_path).filter(|s| !s.is_empty())
}

/// The document's own title, when it is one worth showing: the metadata half
/// of [`display_name`] WITHOUT the path fallback.
///
/// Its own function because a persisted name and a displayed name are not the
/// same question. A shelf record that filled a blank title with the stem of
/// whatever address the open ran on wrote the store's own `source.pdf` stem
/// over a stored book — a layout artifact as a name. What a document supplied
/// is worth persisting; what an address happens to end with is the fallback's
/// business, at the moment it is shown.
pub fn document_title(title: Option<&str>) -> Option<String> {
    title
        .map(str::trim)
        .filter(|t| is_usable_title(t))
        .map(str::to_string)
}

/// True when a title is worth showing instead of the file name: short enough,
/// not URL- or path-shaped, not a known placeholder, not a filename wearing a
/// title's clothes, and not all punctuation.
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

/// The shapes a downloader or a scanner leaves in `/Title`: the name the file
/// arrived with, still wearing its extension ("0321894073.pdf"), a bare
/// ISBN/UPC digit run, or a snake-case mangling with underscores where a
/// person would have typed spaces. In every case the file on disk has usually
/// been renamed since, so the stem of the address is the honest name and the
/// metadata is the stale one.
fn looks_like_file_name(t: &str) -> bool {
    // A title does not carry its own extension.
    if strip_doc_extension(t) != t {
        return true;
    }
    // ISBN-10/13 and UPC digit runs, separators aside. Nine or more, so a
    // real numeric title like "1984" stays a title; the trailing check-digit
    // X of an ISBN-10 counts as a digit.
    let run: String = t.chars().filter(|c| *c != '-' && *c != ' ').collect();
    if run.len() >= 9
        && run
            .chars()
            .all(|c| c.is_ascii_digit() || c == 'x' || c == 'X')
        && run.chars().any(|c| c.is_ascii_digit())
    {
        return true;
    }
    // Snake-case: a title typed by a person has spaces. The exception is a
    // trailing `_N` copy counter — the name file managers give the second of
    // two files that would collide ("dune_1"), and the convention the
    // library's own duplicate naming follows. A name that convention minted
    // has to survive the rule that hunts download debris.
    strip_copy_counter(t).contains('_') && !t.contains(' ')
}

/// Drop a trailing `_N` copy counter ("dune_1" → "dune"), when there is one.
/// One level, on purpose: a counter is appended to a name that had none, so
/// "dune_1_2" is a snake-case name in its own right — and the duplicate namer
/// never mints one (it strips the old counter before appending the next).
///
/// Public because it is ONE rule with two readers, and the two have to agree
/// or the convention breaks: this module reads it to spare a minted name from
/// the download-debris test above, and `library_core::book::duplicate_title`
/// reads it to step a counter instead of stacking one ("Dune_1" → "Dune_2",
/// not "Dune_1_1").
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

/// Extensions this app never admits but a name may still carry: a file the
/// reader was handed before the format gate learned to refuse it, or a title
/// typed by hand. Everything the library can actually hold comes from the
/// format registry instead.
const OFFICE_AND_PRINT: [&str; 7] = ["doc", "docx", "ps", "dvi", "tex", "ppt", "pptx"];

/// Remove a trailing document extension (case-insensitive).
///
/// Read from the format registry rather than typed out here, so a fourth kind
/// strips its own extension with no edit to this list: a shelf that shows
/// "notes.md" under a Markdown book is telling the reader the file system's
/// business, and the card next to it already says what KIND of document this
/// is.
///
/// A title like "Rust 1.75" keeps its ".75": the part after the last dot has
/// to BE a known extension, and "75" is not one. A leading dot is the whole
/// of a hidden file's name (".markdown"), not an extension on an empty stem.
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
        // The /Title a downloader leaves behind — an ISBN wearing its
        // extension, a bare ISBN, a snake-case mangling — loses to the stem of
        // the address, which is the name the file has on disk now.
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
        // Real titles stay titles, short numeric ones included, and a dot in
        // the middle of a name is not an extension.
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
        // A trailing copy counter is the one snake-case shape that IS a name
        // a person (or the library's duplicate namer) chose.
        assert!(!super::looks_like_file_name("dune_1"));
        assert!(!super::looks_like_file_name("Dune_12"));
        // A counter on a mangled name does not launder the mangling.
        assert!(super::looks_like_file_name("harry_potter_goblet_1"));
    }

    #[test]
    fn the_document_title_is_the_metadata_half_alone() {
        // The half of `display_name` a persisted name is made of: what the
        // document supplied, trimmed, and nothing borrowed from an address.
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
            // The formats this app opens, not just the ones pdf.js reads: a
            // Markdown book is named "notes" on the shelf, never "notes.md".
            ("/books/notes.md", Some("notes")),
            ("/books/notes.markdown", Some("notes")),
            ("/books/notes.MDOWN", Some("notes")),
            ("/books/log.txt", Some("log")),
            ("/books/log.text", Some("log")),
            // And the office and print kinds, which the gate refuses but a name
            // may still wear.
            ("/books/slides.pptx", Some("slides")),
            // A version is not an extension, a hidden file's dot is not one
            // either, and an unknown one stays because nothing claims it.
            ("/books/Rust 1.75", Some("Rust 1.75")),
            ("/books/.markdown", Some(".markdown")),
            ("/books/archive.epub", Some("archive.epub")),
            // Lowercasing can expand Unicode. The dot offset must still be
            // measured in the original name that is sliced.
            ("/books/İ.PDF", Some("İ")),
            ("/", None),
        ] {
            assert_eq!(file_stem_from_path(path).as_deref(), want, "path {path:?}");
        }
    }
}
