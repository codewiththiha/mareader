//! What the reader can open — the one registry every entry point consults:
//! the open dialog's filter, the drop target's feedback and the OS handoff
//! all answer the same question. Adding a format is adding a row to
//! [`SUPPORTED`].
//!
//! Two declarations outside this crate derive from that row and cannot see
//! it: the Tauri shell's filesystem gate (`DOCUMENT_EXTENSIONS`) and the
//! bundle's file associations (`tauri.conf.json`). tools/check-formats.ts
//! fails CI when the three disagree.

use serde::{Deserialize, Serialize};

/// One openable document kind: its file extensions (lower-case, no dot), the
/// MIME types a drag may advertise it under before its name is known, and the
/// pipeline it opens through.
///
/// The [`Format`] is a FIELD of the row rather than something recovered from
/// `name` by a match: a match with a catch-all arm answers for a row nobody
/// wrote yet, so a fourth kind would have resolved to [`Format::Pdf`]
/// silently. Carrying it means the table cannot be extended without saying
/// which pipeline the new kind uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentKind {
    pub name: &'static str,
    pub extensions: &'static [&'static str],
    pub mimes: &'static [&'static str],
    pub format: Format,
}

/// Every kind the reader opens.
pub const SUPPORTED: &[DocumentKind] = &[
    DocumentKind {
        name: "PDF",
        extensions: &["pdf"],
        mimes: &["application/pdf", "application/x-pdf"],
        format: Format::Pdf,
    },
    DocumentKind {
        name: "Text",
        extensions: &["txt", "text"],
        mimes: &["text/plain"],
        format: Format::Text,
    },
    DocumentKind {
        name: "Markdown",
        extensions: &["md", "markdown", "mdown"],
        mimes: &["text/markdown", "text/x-markdown"],
        format: Format::Markdown,
    },
];

/// The pipeline a path opens through. PDF renders through the pdf.js engine;
/// the two reflowable formats share `reflow-core`'s maths and differ only in
/// how source becomes blocks (`txt-core`, `md-core`). Page, zoom and
/// navigation machinery is the same for all three, so this enum names
/// pipelines rather than file types: adding a format is a row in [`SUPPORTED`]
/// plus a handler, not a branch in the viewer.
///
/// Ordered and hashable because a persisted library keeps one set of formats
/// per watched folder and one book per fingerprint; serializable because that
/// set is storage. The serde names are the lower-case pipeline names, so a
/// blob reads as `"pdf"` rather than as the variant's capitalisation.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Default,
    Serialize,
    Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Pdf,
    Text,
    Markdown,
}

impl Format {
    /// True for the reflowable formats — the ones with typography settings,
    /// measurement-driven pagination and no pdf.js involvement. Named for
    /// what they share rather than for not being a PDF.
    pub fn is_reflowable(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }

    /// The kind's display name, for sentences that name one document's format
    /// ("Could not open this Markdown"). The registry's `DocumentKind::name`
    /// and this must agree — a test holds them together.
    pub fn label(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Text => "Text",
            Format::Markdown => "Markdown",
        }
    }

    /// The sub-directory the app's store files this format's copies under.
    ///
    /// Its own column rather than the lower-cased [`label`](Self::label): a
    /// display name is copy a writer may reword, and rewording one must not
    /// orphan every copy already on disk. The match is exhaustive, so a fourth
    /// pipeline has to name its own directory here.
    pub fn store_dir(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Text => "text",
            Format::Markdown => "markdown",
        }
    }
}

/// The format a bare extension names, if the registry knows it. Accepts the
/// extension with or without its leading dot, in any case. The one place an
/// extension is turned into a [`Format`]: [`format_of`] and
/// [`is_supported_path`] both answer through it, and the answer is the row's
/// own column, so a fourth kind in [`SUPPORTED`] is admitted by every caller
/// with no edit anywhere else.
pub fn format_from_ext(ext: &str) -> Option<Format> {
    let ext = ext.trim().trim_start_matches('.').to_ascii_lowercase();
    if ext.is_empty() {
        return None;
    }
    SUPPORTED
        .iter()
        .find(|kind| kind.extensions.contains(&ext.as_str()))
        .map(|kind| kind.format)
}

/// The last dotted segment of a path, if it has one. Split on both separators
/// so a Windows path resolves under every host.
fn extension_of(path: &str) -> Option<&str> {
    let name = path.trim_end().rsplit(['/', '\\']).next().unwrap_or("");
    name.rsplit_once('.').map(|(_, ext)| ext)
}

/// The format of a path, by extension. Callers check [`is_supported_path`]
/// first; a name the registry does not know answers PDF (the historical
/// default — and the format an extension-less handoff can only be).
pub fn format_of(path: &str) -> Format {
    extension_of(path)
        .and_then(format_from_ext)
        .unwrap_or(Format::Pdf)
}

/// Every supported extension, flattened (for dialog filters).
pub fn extensions() -> impl Iterator<Item = &'static str> {
    SUPPORTED.iter().flat_map(|kind| kind.extensions.iter().copied())
}

/// Every supported kind's display name, in registry order ("PDF", "Text", ...).
/// UI copy is generated from the registry rather than typed out: a fourth row
/// appears in every sentence that lists the kinds with no edit to any of
/// them.
pub fn kind_names() -> impl Iterator<Item = &'static str> {
    SUPPORTED.iter().map(|kind| kind.name)
}

/// The supported kinds as a reading list: "PDF, Text or Markdown". Two kinds
/// read "A or B", one reads its name alone. No oxford comma — this ends up
/// inside short UI sentences.
pub fn kind_list() -> String {
    let names: Vec<&str> = kind_names().collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_string(),
        _ => format!("{} or {}", names[..names.len() - 1].join(", "), names[names.len() - 1]),
    }
}

/// Whether `path` names a file the reader can open, by extension. Case- and
/// trailing-whitespace-insensitive; a name without an extension is not.
pub fn is_supported_path(path: &str) -> bool {
    extension_of(path).and_then(format_from_ext).is_some()
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
        assert!(is_supported_path("/Users/me/notes.txt"));
        assert!(is_supported_path("/Users/me/notes.MD"));
        assert!(is_supported_path("C:\\docs\\README.markdown"));
        assert!(!is_supported_path("/Users/me/Books/Dune.epub"));
        assert!(!is_supported_path("/Users/me/pdf"));
        assert!(!is_supported_path("notes.pdf.epub"));
        assert!(!is_supported_path(""));
    }

    #[test]
    fn mimes_accept_the_known_kinds_and_the_unknown_blank() {
        assert!(is_supported_mime("application/pdf"));
        assert!(is_supported_mime("Application/PDF"));
        assert!(is_supported_mime("text/plain"));
        assert!(is_supported_mime("text/markdown"));
        assert!(is_supported_mime(""));
        assert!(!is_supported_mime("image/png"));
        assert!(!is_supported_mime("application/epub+zip"));
    }

    #[test]
    fn the_first_openable_path_wins_over_earlier_junk() {
        let paths = ["cover.png", "book.pdf", "other.pdf"];
        assert_eq!(first_supported(paths), Some("book.pdf"));
        assert_eq!(first_supported(["a.png", "b.epub"]), None);
        assert_eq!(first_supported(["a.png", "notes.txt"]), Some("notes.txt"));
    }

    #[test]
    fn the_format_follows_the_extension() {
        assert_eq!(format_of("/books/dune.pdf"), Format::Pdf);
        assert_eq!(format_of("/books/notes.TXT"), Format::Text);
        assert_eq!(format_of("/books/notes.text"), Format::Text);
        assert_eq!(format_of("/books/README.md"), Format::Markdown);
        assert_eq!(format_of("/books/notes.mdown"), Format::Markdown);
        // Unknown or missing extensions fall back to the default pipeline.
        assert_eq!(format_of("/books/notes.epub"), Format::Pdf);
        assert_eq!(format_of("/books/Makefile"), Format::Pdf);
        assert_eq!(format_of("C:\\books\\chapter.MARKDOWN"), Format::Markdown);
    }

    #[test]
    fn an_extension_alone_resolves_the_same_way_a_path_does() {
        assert_eq!(format_from_ext("pdf"), Some(Format::Pdf));
        assert_eq!(format_from_ext(".PDF"), Some(Format::Pdf));
        assert_eq!(format_from_ext("text"), Some(Format::Text));
        assert_eq!(format_from_ext("mdown"), Some(Format::Markdown));
        assert_eq!(format_from_ext("epub"), None);
        assert_eq!(format_from_ext("."), None);
        assert_eq!(format_from_ext(""), None);
        // Every registry row resolves through the one column that answers it.
        for kind in SUPPORTED {
            for ext in kind.extensions {
                assert_eq!(
                    format_from_ext(ext),
                    Some(kind.format),
                    "{ext} must map back to its own row"
                );
                // The row's display name and the pipeline's label are one fact
                // spelled in two places; a reword of either has to move both.
                assert_eq!(
                    kind.format.label(),
                    kind.name,
                    "{ext}: the registry's name and Format::label disagree"
                );
            }
        }
    }

    #[test]
    fn every_pipeline_names_its_own_store_directory() {
        // Rewording "Markdown" must not orphan every copy already on disk
        // under `markdown/`.
        assert_eq!(Format::Pdf.store_dir(), "pdf");
        assert_eq!(Format::Text.store_dir(), "text");
        assert_eq!(Format::Markdown.store_dir(), "markdown");
        for kind in SUPPORTED {
            assert!(
                !kind.format.store_dir().is_empty(),
                "{} has no store directory",
                kind.name
            );
        }
    }

    #[test]
    fn a_format_persists_under_its_pipeline_name() {
        // The library blob stores a format per book and a set per watched
        // folder; the names are storage, so they are the lower-case pipeline
        // names rather than the variant's capitalisation.
        assert_eq!(serde_json::to_string(&Format::Pdf).unwrap(), "\"pdf\"");
        assert_eq!(serde_json::to_string(&Format::Text).unwrap(), "\"text\"");
        assert_eq!(
            serde_json::to_string(&Format::Markdown).unwrap(),
            "\"markdown\""
        );
        let back: Format = serde_json::from_str("\"markdown\"").unwrap();
        assert_eq!(back, Format::Markdown);
        assert!(serde_json::from_str::<Format>("\"epub\"").is_err());
    }

    #[test]
    fn the_kind_list_is_read_out_of_the_registry() {

        // UI copy is generated, never typed: a row added to `SUPPORTED` shows
        // up here with no edit.
        assert_eq!(kind_list(), "PDF, Text or Markdown");
        assert_eq!(kind_names().count(), SUPPORTED.len());
        // Every extension resolves to a kind whose label is in that list, so no
        // document can fail with a sentence that does not name its own format.
        for kind in SUPPORTED {
            for ext in kind.extensions {
                let path = format!("/books/sample.{ext}");
                assert!(kind_list().contains(format_of(&path).label()));
            }
        }
    }
}
