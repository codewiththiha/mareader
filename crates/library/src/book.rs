//! A book, where its file is, and where its reader is in it.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp, stamped_or_bare};
use crate::extra::Extra;
use crate::id::{BookId, ContentId, FolderId};

/// The file's own format; text and Markdown open converted to EPUB.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Format {
    Pdf,
    Epub,
    Text,
    Markdown,
}

impl Format {
    pub const ALL: [Self; 4] = [Self::Pdf, Self::Epub, Self::Text, Self::Markdown];

    /// By the file's extension, any case; none for other files.
    pub fn of_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        let ext = ext.as_str();
        Self::ALL
            .into_iter()
            .find(|f| f.extensions().contains(&ext))
    }

    /// The lowercase file extensions this format is read from.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::Pdf => &["pdf"],
            Self::Epub => &["epub"],
            Self::Text => &["txt", "text"],
            Self::Markdown => &["md", "markdown", "mdown"],
        }
    }

    /// Text is hashed whole: a same-length edit can miss both ends.
    pub fn hashed_whole(self) -> bool {
        matches!(self, Self::Text | Self::Markdown)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "PDF",
            Self::Epub => "EPUB",
            Self::Text => "TXT",
            Self::Markdown => "MD",
        }
    }
}

/// When a book's own design shows, set for that book alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Design {
    /// In the chapters it designs.
    Auto,
    On,
    Off,
}

/// A durable position within one EPUB spine item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpinePosition {
    pub href: String,
    pub item: usize,
    pub block: usize,
    pub offset: usize,
}

/// Where a reader was: the page shown, and a book's place in its text.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub page: usize,
    /// The page count then, for "page 12 of 300" and the progress.
    pub pages: usize,
    /// A global block anchor for positions saved by older readers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<(usize, usize)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spine: Option<SpinePosition>,
}

impl Position {
    /// How far through, 0 to 1; none before the count is known.
    pub fn fraction(&self) -> Option<f32> {
        let read = self.page.saturating_add(1) as f32;
        (self.pages > 0).then(|| (read / self.pages as f32).min(1.0))
    }
}

/// What changes as a book is read; kept in its own small file.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct State {
    pub position: Field<Position>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl State {
    pub fn new(position: Field<Position>) -> Self {
        Self {
            position,
            extra: Extra::default(),
        }
    }
}

/// Size and modification time, to skip re-hashing an unchanged file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStamp {
    pub size: u64,
    pub mtime_ms: u64,
}

/// How the library came to know a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    /// Opened in the reader and read in place; never moved or deleted.
    Opened,
    /// Found in a folder the library reads in place; never touched.
    #[serde(alias = "source")]
    Folder(FolderId),
    /// Copied into the library's own folder; deleted with the book.
    Store,
}

/// One place a book's file is; local to this machine, never synced.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Location {
    pub path: PathBuf,
    pub origin: Origin,
    pub stamp: FileStamp,
    /// For the library's own copy: the file it was copied from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<PathBuf>,
    /// Whether the file was there when last looked for; not saved.
    #[serde(skip, default = "present")]
    pub present: bool,
}

fn present() -> bool {
    true
}

/// One book: a content, its names, and where its file may be found.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Book {
    pub id: BookId,
    pub content: ContentId,
    pub format: Format,
    pub title: Field<String>,
    pub author: Field<Option<String>>,
    /// When it joined the library, in wall milliseconds.
    pub added: u64,
    /// When it was last opened, in wall milliseconds.
    pub opened: Field<u64>,
    /// Its place on the top level, among shelves and books; see `arrange`.
    #[serde(default)]
    pub key: Field<String>,
    /// Newest first; the first present one is opened. Local, not synced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<Location>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub removed: Option<Stamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restored: Option<Stamp>,
    /// The book this one is read separately from; they share its files.
    #[serde(
        default,
        deserialize_with = "stamped_or_bare",
        skip_serializing_if = "Field::is_unset"
    )]
    pub twin_of: Field<Option<BookId>>,
    /// Kept as new beside another of its content: lookups pass it by.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub independent: bool,
    /// Its own design choice; none follows the reader's setting.
    #[serde(default, skip_serializing_if = "Field::is_unset")]
    pub design: Field<Option<Design>>,
    /// Removed with its data: a return lands as a new book.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forgot: Option<Stamp>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// Whether a restoration postdates a removal tombstone.
pub(crate) fn alive(removed: Option<Stamp>, restored: Option<Stamp>) -> bool {
    match (removed, restored) {
        (None, _) => true,
        (Some(removed), Some(restored)) => restored > removed,
        (Some(_), None) => false,
    }
}

impl Book {
    /// Not removed, or restored since; removal is a tombstone.
    pub fn alive(&self) -> bool {
        alive(self.removed, self.restored)
    }

    /// The file to open: the newest location still there.
    pub fn path(&self) -> Option<&Path> {
        let here = self.locations.iter().find(|l| l.present);
        here.map(|l| l.path.as_path())
    }

    /// The book whose files it shares: itself unless a twin.
    pub fn original(&self) -> BookId {
        self.twin_of.value.unwrap_or(self.id)
    }

    /// No location of it is there any more.
    pub fn missing(&self) -> bool {
        self.path().is_none()
    }

    /// Whether it is the one book its content names: no twin or own copy.
    pub fn shared(&self) -> bool {
        self.twin_of.value.is_none() && !self.independent
    }

    /// Its newest location: the file it reads, there or not.
    pub fn home(&self) -> Option<&Location> {
        self.locations
            .iter()
            .find(|l| l.present)
            .or(self.locations.first())
    }
}

/// Titles that name the program or template, never the book.
const GENERIC: [&str; 6] = [
    "untitled",
    "document",
    "microsoft word",
    "powerpoint presentation",
    "slide 1",
    "title",
];

/// The document's own title, unless generic; else the file's name.
pub fn display_title(own: Option<&str>, path: &Path) -> String {
    let stem = path.file_stem().map(|s| s.to_string_lossy());
    let name = stem.as_deref().map_or("", |s| without_copy_mark(s.trim()));
    let own = own
        .map(str::trim)
        .filter(|t| !t.is_empty() && !generic(t, name));
    match (own, name.is_empty()) {
        (Some(title), _) => title.to_owned(),
        (None, false) => name.to_owned(),
        (None, true) => "Untitled".to_owned(),
    }
}

/// A template's title, or the file's name in poorer capitals.
fn generic(title: &str, name: &str) -> bool {
    let lower = title.to_lowercase();
    let template = GENERIC
        .iter()
        .any(|g| lower == *g || lower.starts_with(&format!("{g} -")));
    let capitals = |s: &str| s.chars().filter(|c| c.is_uppercase()).count();
    let worse_case = lower == name.to_lowercase() && capitals(name) > capitals(title);
    template || worse_case
}

/// `book (1)` and `book copy` are `book`; a lone `(1)` stays.
fn without_copy_mark(name: &str) -> &str {
    let trimmed = name.trim_end_matches(|c: char| c.is_ascii_digit() || c == ' ');
    let marks = [trimmed.strip_suffix(" copy"), copy_number(name)];
    marks
        .into_iter()
        .flatten()
        .map(str::trim_end)
        .find(|s| !s.is_empty())
        .unwrap_or(name)
}

/// `book (12)` is `book`.
fn copy_number(name: &str) -> Option<&str> {
    let open = name.strip_suffix(')')?.rfind('(')?;
    let digits = name.get(open + 1..name.len() - 1)?;
    let numbered = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
    numbered.then(|| name.get(..open)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_fall_back_to_the_files_name_without_its_copy_mark() {
        let path = |p: &str| PathBuf::from(p);
        let t = |own: Option<&str>, p: &str| display_title(own, &path(p));
        assert_eq!(t(Some("Moby Dick"), "/b/x.pdf"), "Moby Dick");
        assert_eq!(t(None, "/b/Moby Dick (1).pdf"), "Moby Dick");
        assert_eq!(t(Some("  "), "/b/notes copy.md"), "notes");
        assert_eq!(
            t(Some("Microsoft Word - Document1"), "/b/Essay.pdf"),
            "Essay"
        );
        assert_eq!(t(Some("untitled"), "/b/(1).pdf"), "(1)");
        assert_eq!(t(Some("essay"), "/b/Essay.pdf"), "Essay");
        assert_eq!(t(Some("Covered"), "/b/covered.epub"), "Covered");
        assert_eq!(t(None, "/b/.pdf"), ".pdf");
        assert_eq!(Format::of_path(&path("/a/B.MD")), Some(Format::Markdown));
        assert_eq!(Format::of_path(&path("/a/b.docx")), None);
    }

    #[test]
    fn old_saved_positions_load_without_a_spine_locator() {
        let old = r#"{"page":7,"pages":30,"place":[12,4]}"#;
        let position = serde_json::from_str::<Position>(old).ok();
        assert_eq!(position.map(|p| p.spine), Some(None));
    }

    #[test]
    fn progress_counts_the_page_shown_as_read() {
        let at = |page, pages| {
            Position {
                page,
                pages,
                place: None,
                spine: None,
            }
            .fraction()
        };
        assert_eq!(at(0, 0), None);
        assert_eq!(at(0, 4), Some(0.25));
        assert_eq!(at(3, 4), Some(1.0));
        // A count that shrank since never reads past the end.
        assert_eq!(at(9, 4), Some(1.0));
    }
}
