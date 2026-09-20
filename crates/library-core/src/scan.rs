//! What a folder scan found, and whether the folder's own options admit it.
//!
//! The shell walks the filesystem and produces [`FoundFile`] rows; this module
//! decides which of them the configured folder wants. The decision is pure, so
//! its awkward corners — the include/exclude flip and the size threshold's
//! strictness — are host-testable.

use serde::{Deserialize, Serialize};

use document_core::format::{Format, SUPPORTED, format_from_ext};

use crate::book::Fingerprint;
use crate::folder::FolderOpts;

/// One file a walk turned up. Serialized: the shell produces these and the
/// frontend's ledger consumes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FoundFile {
    pub path: String,
    /// Relative to the watched root, `/`-separated on every platform; empty for a file at the root itself.
    pub rel: String,
    pub ext: String,
    pub size: u64,
    pub fp: Fingerprint,
}

impl FoundFile {
    pub fn format(&self) -> Option<Format> {
        format_from_ext(&self.ext)
    }

    /// The format the file was admitted by. A file the registry refused never
    /// reaches a mint, so a `None` here means a hand-built [`FoundFile`]; the
    /// fallback is named once rather than scattered `unwrap_or` shields at
    /// every mint.
    pub fn admitted_format(&self) -> Format {
        self.format().unwrap_or(Format::Pdf)
    }

    pub fn subfolder(&self) -> &str {
        subfolder_of(&self.rel)
    }
}

/// Free function rather than a [`FoundFile`] method: a book already in the
/// library has an address and no finding, and the two must agree about which
/// rung an address belongs to — see
/// [`crate::folder::WatchedFolder::rungs_for`].
pub fn subfolder_of(rel: &str) -> &str {
    match rel.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "",
    }
}

/// Whether a folder's options admit one found file. `min_size` is a strict
/// lower bound: "larger than 30 KB" rejects exactly 30 KB, which is what the
/// import sheet's wording promises.
pub fn admits(opts: &FolderOpts, ext: &str, size: u64) -> bool {
    let Some(fmt) = format_from_ext(ext) else {
        return false;
    };
    let selected = opts.formats.contains(&fmt);
    let wanted = if opts.include_selected { selected } else { !selected };
    wanted && size > opts.min_size
}

/// The formats a folder may select, straight out of the registry and in its order.
pub fn selectable_formats() -> Vec<Format> {
    SUPPORTED.iter().map(|kind| kind.format).collect()
}

/// The store sub-directory a format's copies go into: the pipeline's own
/// name, so the directories read `pdf`, `text` and `markdown`.
pub fn store_dir(ext: &str) -> &'static str {
    format_from_ext(ext)
        .map_or("other", |fmt| fmt.store_dir())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn opts(formats: &[Format], include: bool, min: u64) -> FolderOpts {
        FolderOpts {
            formats: formats.iter().copied().collect::<BTreeSet<_>>(),
            include_selected: include,
            min_size: min,
            ..FolderOpts::default()
        }
    }

    fn found(path: &str, rel: &str, size: u64) -> FoundFile {
        FoundFile {
            path: path.to_string(),
            rel: rel.to_string(),
            ext: path.rsplit('.').next().unwrap_or("").to_string(),
            size,
            fp: Fingerprint {
                size,
                mtime_ms: 1,
                head_hash: 1,
            },
        }
    }

    #[test]
    fn a_selected_format_is_admitted_and_an_unselected_one_is_not() {
        let o = opts(&[Format::Pdf], true, 0);
        assert!(admits(&o, "pdf", 1));
        assert!(!admits(&o, "md", 1));
        assert!(!admits(&o, "txt", 1));
    }

    #[test]
    fn excluding_the_selection_admits_everything_else() {
        let o = opts(&[Format::Pdf], false, 0);
        assert!(!admits(&o, "pdf", 1));
        assert!(admits(&o, "md", 1));
        assert!(admits(&o, "txt", 1));
        assert!(!admits(&o, "epub", 1));
    }

    #[test]
    fn the_size_threshold_is_strict() {
        let o = opts(&[Format::Pdf], true, 30 * 1024);
        assert!(!admits(&o, "pdf", 30 * 1024), "exactly 30 KB is not larger than 30 KB");
        assert!(admits(&o, "pdf", 30 * 1024 + 1));
        assert!(!admits(&o, "pdf", 1));
        // Zero is strict too: "larger than zero" refuses an empty file.
        assert!(admits(&opts(&[Format::Pdf], true, 0), "pdf", 1));
        assert!(!admits(&opts(&[Format::Pdf], true, 0), "pdf", 0));
    }

    #[test]
    fn an_unknown_extension_is_refused_whatever_the_options() {
        for include in [true, false] {
            let o = opts(&[Format::Pdf, Format::Text, Format::Markdown], include, 0);
            assert!(!admits(&o, "epub", 1 << 20));
            assert!(!admits(&o, "", 1 << 20));
            assert!(!admits(&o, "png", 1 << 20));
        }
    }

    #[test]
    fn extensions_are_matched_case_blindly_and_with_their_dot() {
        let o = opts(&[Format::Markdown], true, 0);
        assert!(admits(&o, "MD", 1));
        assert!(admits(&o, ".md", 1));
        assert!(admits(&o, "markdown", 1));
        assert!(admits(&o, "mdown", 1));
    }

    #[test]
    fn the_store_directories_are_the_pipelines_own() {
        assert_eq!(store_dir("pdf"), "pdf");
        assert_eq!(store_dir("TXT"), "text");
        assert_eq!(store_dir("markdown"), "markdown");
        assert_eq!(store_dir("mdown"), "markdown");
        assert_eq!(store_dir("epub"), "other", "an unknown kind gets no directory of its own");
    }

    #[test]
    fn the_selectable_set_is_the_registry_and_nothing_more() {
        let formats = selectable_formats();
        assert_eq!(formats.len(), SUPPORTED.len());
        assert_eq!(formats, vec![Format::Pdf, Format::Text, Format::Markdown]);
    }

    #[test]
    fn the_subfolder_is_the_relative_path_minus_its_name() {
        assert_eq!(found("/r/a.pdf", "a.pdf", 1).subfolder(), "");
        assert_eq!(found("/r/x/a.pdf", "x/a.pdf", 1).subfolder(), "x");
        assert_eq!(found("/r/x/y/a.pdf", "x/y/a.pdf", 1).subfolder(), "x/y");
        // A Windows walk normalises its separators to `/` before this sees it.
        assert_eq!(found("C:\\r\\x\\a.pdf", "x\\a.pdf", 1).subfolder(), "");
    }

    #[test]
    fn a_found_file_crosses_the_wire_with_its_fingerprint() {
        let f = found("/r/a.pdf", "a.pdf", 1234);
        let json = serde_json::to_string(&f).unwrap();
        assert!(json.contains("\"mtimeMs\""), "{json}");
        assert!(json.contains("\"headHash\""), "{json}");
        let back: FoundFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }
}
