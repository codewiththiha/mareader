//! What the reader can open — the one registry every entry point consults.
//!
//! The open dialog's filter, the drop target's "is this worth showing feedback
//! for" and the OS handoff all answer the same question, so the answer lives
//! here once. Adding a format is adding a row to [`SUPPORTED`].

/// One openable document kind: its file extensions (lower-case, no dot) and
/// the MIME types a drag may advertise it under before its name is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentKind {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub mimes: &'static [&'static str],
}

/// Every kind the reader opens.
pub const SUPPORTED: &[DocumentKind] = &[DocumentKind {
    name: "PDF",
    extensions: &["pdf"],
    mimes: &["application/pdf", "application/x-pdf"],
}];

/// Every supported extension, flattened (for dialog filters).
pub fn extensions() -> impl Iterator<Item = &'static str> {
    SUPPORTED.iter().flat_map(|kind| kind.extensions.iter().copied())
}

/// Whether `path` names a file the reader can open, by extension. Case- and
/// trailing-whitespace-insensitive; a name without an extension is not.
pub fn is_supported_path(path: &str) -> bool {
    let name = path.trim_end().rsplit(['/', '\\']).next().unwrap_or("");
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let ext = ext.to_ascii_lowercase();
    extensions().any(|known| known == ext)
}

/// Whether a drag advertising `mime` may be carrying a supported document.
/// An EMPTY type is accepted: browsers omit it for files whose kind they do
/// not know at drag time, and the drop itself is still checked by path.
pub fn is_supported_mime(mime: &str) -> bool {
    mime.is_empty()
        || SUPPORTED
            .iter()
            .any(|kind| kind.mimes.iter().any(|m| m.eq_ignore_ascii_case(mime)))
}

/// The first openable path among `paths`, if any.
pub fn first_supported<'a, I: IntoIterator<Item = &'a str>>(paths: I) -> Option<&'a str> {
    paths.into_iter().find(|path| is_supported_path(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_matched_by_extension_only() {
        assert!(is_supported_path("/Users/me/Books/Dune.pdf"));
        assert!(is_supported_path("C:\\books\\DUNE.PDF"));
        assert!(is_supported_path("weird.name.with.dots.Pdf "));
        assert!(!is_supported_path("/Users/me/Books/Dune.epub"));
        assert!(!is_supported_path("/Users/me/pdf"));
        assert!(!is_supported_path("notes.pdf.txt"));
        assert!(!is_supported_path(""));
    }

    #[test]
    fn mimes_accept_the_known_kinds_and_the_unknown_blank() {
        assert!(is_supported_mime("application/pdf"));
        assert!(is_supported_mime("Application/PDF"));
        assert!(is_supported_mime(""));
        assert!(!is_supported_mime("image/png"));
        assert!(!is_supported_mime("text/plain"));
    }

    #[test]
    fn the_first_openable_path_wins_over_earlier_junk() {
        let paths = ["cover.png", "book.pdf", "other.pdf"];
        assert_eq!(first_supported(paths), Some("book.pdf"));
        assert_eq!(first_supported(["a.png", "b.txt"]), None);
    }
}
