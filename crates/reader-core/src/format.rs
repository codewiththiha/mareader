//! What the reader can open: one registry every entry point consults.

use serde::{Deserialize, Serialize};

/// One openable document kind: extensions, MIME types and pipeline.
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

/// The pipeline a path opens through: pdf.js, or the reflowable pair.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    #[default]
    Pdf,
    Text,
    Markdown,
}

impl Format {
    /// True for the reflowable formats, which carry typography settings.
    pub fn is_reflowable(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }

    /// The kind's display name, for sentences naming a format.
    pub fn label(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Text => "Text",
            Format::Markdown => "Markdown",
        }
    }

    /// The sub-directory the app's store files this format's copies under.
    pub fn store_dir(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Text => "text",
            Format::Markdown => "markdown",
        }
    }
}

/// The format a bare extension names, if the registry knows it.
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

/// The last dotted segment of a path, if it has one.
fn extension_of(path: &str) -> Option<&str> {
    let name = path.trim_end().rsplit(['/', '\\']).next().unwrap_or("");
    name.rsplit_once('.').map(|(_, ext)| ext)
}

/// The format of a path, by extension; unknown names answer PDF.
pub fn format_of(path: &str) -> Format {
    extension_of(path)
        .and_then(format_from_ext)
        .unwrap_or(Format::Pdf)
}

/// Every supported extension, flattened (for dialog filters).
pub fn extensions() -> impl Iterator<Item = &'static str> {
    SUPPORTED
        .iter()
        .flat_map(|kind| kind.extensions.iter().copied())
}

/// Every supported kind's display name, in registry order.
pub fn kind_names() -> impl Iterator<Item = &'static str> {
    SUPPORTED.iter().map(|kind| kind.name)
}

/// The supported kinds as a reading list, e.g. "PDF, Text or
/// Markdown".
pub fn kind_list() -> String {
    let names: Vec<&str> = kind_names().collect();
    match names.len() {
        0 => String::new(),
        1 => names[0].to_string(),
        _ => format!(
            "{} or {}",
            names[..names.len() - 1].join(", "),
            names[names.len() - 1]
        ),
    }
}

/// Whether `path` names a file the reader can open.
pub fn is_supported_path(path: &str) -> bool {
    extension_of(path).and_then(format_from_ext).is_some()
}

/// Whether a drag advertising `mime` may carry a supported document.
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
                // The row's display name and the pipeline's label are one fact.
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
        // The library blob stores a format per book and per folder.
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
        // UI copy is generated, never typed.
        assert_eq!(kind_list(), "PDF, Text or Markdown");
        assert_eq!(kind_names().count(), SUPPORTED.len());
        // Every extension resolves to a kind whose label is in that list.
        for kind in SUPPORTED {
            for ext in kind.extensions {
                let path = format!("/books/sample.{ext}");
                assert!(kind_list().contains(format_of(&path).label()));
            }
        }
    }
}
