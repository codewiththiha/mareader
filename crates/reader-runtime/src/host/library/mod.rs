//! The rail's Library panel: the persisted library as a compact tree — its
//! folders (shelves, nested as the library nests them) and its files, one
//! line each, a name and a format badge — and, above it while the workspace
//! is split, the open panes as tabs.
//!
//! It is NOT the library. It reads the persisted blob when it is shown
//! (`storage::load_library`, the reader's read-only view of the store) and
//! edits nothing: no covers, no sorting menu, no import. Its one job is to
//! be where a split comes from:
//!
//! * a file row dragged onto the workspace opens it in a split, through the
//!   host's drag session and workspace command ([`super::drag`],
//!   [`super::commands`]) — the only split-drag source there is;
//! * a click does what the Workspace setting says
//!   ([`reader_core::settings::LibraryClick`]);
//! * an open tab is focused by a click and closed by its ×; tabs never drag.
//!
//! The model below is pure (a library blob in, rows out) so the tree's
//! shape is unit-tested; [`view`] renders it. The state that must outlive
//! the rail — the tree as last read, which folders are open, the scroll
//! offset — is the host's ([`LibraryState`]): the rail remounts whenever
//! the active pane changes.

mod view;

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;
use library_core::blob::LibraryBlob;
use library_core::book::{Book, Row};
use library_core::shelf::{ALL_SHELF, Shelf, members_of};

use super::ReaderHost;
use super::model::{PaneBounds, PaneFormat, PaneId};
use super::tree::{SplitAxis, split_fits};

/// One file row: a book the library holds, or a link to one (shown under
/// the link's name, opening the book it points at).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryFile {
    /// Unique among the tree's rows: a book filed on two shelves is two
    /// rows.
    pub key: String,
    /// The book the row opens (a link's target).
    pub book_id: String,
    pub name: String,
    pub path: String,
    /// The short format tag the row wears (`PDF`, `MD`, `TXT`).
    pub badge: String,
    /// The library's path check lost the file: shown, never opened.
    pub missing: bool,
}

/// One folder: a shelf and what hangs under it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LibraryFolder {
    pub id: String,
    pub name: String,
    pub folders: Vec<LibraryFolder>,
    pub files: Vec<LibraryFile>,
}

impl LibraryFolder {
    /// Every file in this folder and under it.
    pub fn count(&self) -> usize {
        self.files.len() + self.folders.iter().map(LibraryFolder::count).sum::<usize>()
    }
}

/// The library as the panel shows it: the root level's folders and its
/// unfiled files.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct LibraryTree {
    pub root: LibraryFolder,
}

impl LibraryTree {
    pub fn is_empty(&self) -> bool {
        self.root.folders.is_empty() && self.root.files.is_empty()
    }

    /// The tree a library blob describes. Folders first, then files, each
    /// by name (case-insensitive) — an explorer's order, not the shelf's
    /// hand order: the panel is for finding a file, not arranging one.
    pub fn from_blob(blob: &LibraryBlob) -> Self {
        let books: HashMap<&str, &Book> = blob
            .books
            .iter()
            .filter_map(Row::book)
            .map(|book| (book.id.as_str(), book))
            .collect();
        let rows: HashMap<&str, &Row> = blob.books.iter().map(|row| (row.id(), row)).collect();
        let mut seen = HashSet::new();
        let mut root = level(blob, &books, &rows, None, &mut seen);
        root.id = ALL_SHELF.to_string();
        Self { root }
    }
}

/// One level: `shelf` (`None` for the root), its sub-shelves and its
/// members. `seen` guards a shelf cycle (a blob can carry one; the walk
/// must not hang on it).
fn level(
    blob: &LibraryBlob,
    books: &HashMap<&str, &Book>,
    rows: &HashMap<&str, &Row>,
    shelf: Option<&Shelf>,
    seen: &mut HashSet<String>,
) -> LibraryFolder {
    let id = shelf.map_or(ALL_SHELF, |s| s.id.as_str());
    let parent = shelf.map(|s| s.id.as_str());
    let children: Vec<&Shelf> = blob
        .shelves
        .iter()
        .filter(|s| s.parent.as_deref() == parent && seen.insert(s.id.clone()))
        .collect();
    let mut folders: Vec<LibraryFolder> = children
        .into_iter()
        .map(|child| level(blob, books, rows, Some(child), seen))
        .collect();
    folders.sort_by_cached_key(|f| f.name.to_lowercase());
    let mut files: Vec<LibraryFile> = members_of(&blob.books, &blob.shelves, id)
        .into_iter()
        .filter_map(|member| file_of(books, rows, id, member))
        .collect();
    files.sort_by_cached_key(|f| f.name.to_lowercase());
    LibraryFolder {
        id: id.to_string(),
        name: shelf.map(|s| s.name.clone()).unwrap_or_default(),
        folders,
        files,
    }
}

/// The file row for member `member` of level `level` (none for a link whose
/// target is gone).
fn file_of(
    books: &HashMap<&str, &Book>,
    rows: &HashMap<&str, &Row>,
    level: &str,
    member: &str,
) -> Option<LibraryFile> {
    let (name, book) = match rows.get(member)? {
        Row::Book(book) => (book.title(), book),
        Row::Link { name, target, .. } => (name.clone(), *books.get(target.as_str())?),
    };
    Some(LibraryFile {
        key: format!("{level}/{member}"),
        book_id: book.id.clone(),
        name,
        path: book.path().to_string(),
        badge: badge_of(book.path()),
        missing: book.missing,
    })
}

/// The short format tag for an address: its extension, uppercased, with the
/// long spellings shortened. Read from the address — the panel shows what
/// the file is called, it does not classify it.
pub fn badge_of(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let ext = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_lowercase(),
        _ => return String::new(),
    };
    match ext.as_str() {
        "markdown" | "mdown" | "mkd" => "MD".to_string(),
        "text" => "TXT".to_string(),
        other => other.chars().take(4).collect::<String>().to_uppercase(),
    }
}

/// The badge for a pane's format (the tabs strip: a pane names its format,
/// not its address).
pub fn badge_of_format(format: PaneFormat) -> &'static str {
    match format {
        PaneFormat::Pdf => "PDF",
        PaneFormat::Markdown => "MD",
        PaneFormat::Text => "TXT",
        PaneFormat::Pending => "",
    }
}

/// One visible line of the tree, in the order it is drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Line {
    Folder {
        id: String,
        name: String,
        depth: usize,
        open: bool,
        count: usize,
    },
    File {
        file: LibraryFile,
        depth: usize,
    },
}

impl Line {
    /// The line's identity for a keyed list: its id AND what it shows, so a
    /// renamed or recounted line is redrawn (a folder's open state is not
    /// part of it — the row follows that itself).
    pub fn key(&self) -> String {
        match self {
            Line::Folder {
                id,
                name,
                depth,
                count,
                ..
            } => format!("d:{id}|{depth}|{count}|{name}"),
            Line::File { file, depth } => format!(
                "f:{}|{depth}|{}|{}|{}",
                file.key, file.missing, file.path, file.name
            ),
        }
    }
}

/// The lines a tree shows with the folders in `open` expanded: a closed
/// folder's contents are not lines at all.
pub fn lines(tree: &LibraryTree, open: &HashSet<String>) -> Vec<Line> {
    let mut out = Vec::new();
    push_level(&tree.root, 0, open, &mut out);
    out
}

fn push_level(folder: &LibraryFolder, depth: usize, open: &HashSet<String>, out: &mut Vec<Line>) {
    for child in &folder.folders {
        let is_open = open.contains(&child.id);
        out.push(Line::Folder {
            id: child.id.clone(),
            name: child.name.clone(),
            depth,
            open: is_open,
            count: child.count(),
        });
        if is_open {
            push_level(child, depth + 1, open, out);
        }
    }
    for file in &folder.files {
        out.push(Line::File {
            file: file.clone(),
            depth,
        });
    }
}

/// Where an activated row's document goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpenHow {
    /// The focused pane, in place.
    Here,
    /// A new pane beside the focused one.
    Beside,
}

/// The axis a row opened "beside" the focused pane splits it along: to the
/// right when that leaves both halves usable, else below when that does
/// (a narrow column beside the docked rail still takes a split), else to
/// the right so the layout's own "no room" refusal is what the user hears.
/// A pane not measured yet splits right: nothing to judge it by.
pub fn beside_axis(bounds: Option<PaneBounds>) -> SplitAxis {
    match bounds {
        Some(b)
            if b.width > 0.0
                && b.height > 0.0
                && !split_fits(b, SplitAxis::Horizontal)
                && split_fits(b, SplitAxis::Vertical) =>
        {
            SplitAxis::Vertical
        }
        _ => SplitAxis::Horizontal,
    }
}

/// One open pane, as the tabs strip lists it: its id and what its document
/// is called (the strip reads the active id and the format itself, so a
/// focus change restyles a tab rather than rebuilding it).
#[derive(Clone, Copy)]
pub struct OpenTab {
    pub id: PaneId,
    pub name: Signal<String>,
}

/// The panel's host-owned state (see the module docs). Copy handles; the
/// session's owner disposes them with the workspace.
#[derive(Clone, Copy)]
pub struct LibraryState {
    tree: RwSignal<Option<LibraryTree>>,
    open: RwSignal<HashSet<String>>,
    scroll: StoredValue<i32>,
    /// A drag just ended on a row: the click its release produces there is
    /// not a click.
    swallow: StoredValue<bool>,
}

impl LibraryState {
    pub(super) fn new() -> Self {
        Self {
            tree: RwSignal::new(None),
            open: RwSignal::new(HashSet::new()),
            scroll: StoredValue::new(0),
            swallow: StoredValue::new(false),
        }
    }

    /// Re-read the library from the store. Subscribers hear of it only when
    /// the tree changed.
    pub fn refresh(&self) {
        let next = LibraryTree::from_blob(&storage::load_library());
        self.tree.try_maybe_update(|tree| {
            let changed = tree.as_ref() != Some(&next);
            if changed {
                *tree = Some(next);
            }
            (changed, ())
        });
    }

    /// The visible lines (tracked).
    pub fn lines(&self) -> Vec<Line> {
        let open = self.open.try_get().unwrap_or_default();
        self.tree
            .try_with(|tree| tree.as_ref().map(|tree| lines(tree, &open)))
            .flatten()
            .unwrap_or_default()
    }

    /// The library holds nothing (tracked; `false` until first read).
    pub fn is_empty(&self) -> bool {
        self.tree
            .try_with(|tree| tree.as_ref().is_some_and(LibraryTree::is_empty))
            .unwrap_or(false)
    }

    /// Whether `folder` is expanded (tracked).
    pub fn is_open(&self, folder: &str) -> bool {
        self.open
            .try_with(|open| open.contains(folder))
            .unwrap_or(false)
    }

    pub fn toggle(&self, folder: &str) {
        self.open.try_update(|open| {
            if !open.remove(folder) {
                open.insert(folder.to_string());
            }
        });
    }

    pub fn set_open(&self, folder: &str, want: bool) {
        self.open.try_maybe_update(|open| {
            let changed = if want {
                open.insert(folder.to_string())
            } else {
                open.remove(folder)
            };
            (changed, ())
        });
    }

    pub fn scroll(&self) -> i32 {
        self.scroll.try_get_value().unwrap_or(0)
    }

    pub fn remember_scroll(&self, top: i32) {
        self.scroll.try_set_value(top);
    }

    pub(super) fn swallow_click(&self) {
        self.swallow.try_set_value(true);
    }

    /// Whether the click arriving now follows a drag (and clear it).
    pub fn take_swallowed(&self) -> bool {
        self.swallow
            .try_update_value(std::mem::take)
            .unwrap_or(false)
    }

    pub fn clear_swallowed(&self) {
        self.swallow.try_set_value(false);
    }
}

/// The handle the active pane's rail finds in context to show the panel.
#[derive(Clone, Copy)]
pub struct LibraryPanel {
    host: ReaderHost,
}

impl LibraryPanel {
    pub(super) fn new(host: ReaderHost) -> Self {
        Self { host }
    }

    /// The panel, for a rail slot. `shown`: the panel is the rail's visible
    /// one (it reads the store when it becomes so).
    pub fn view(self, shown: Signal<bool>) -> AnyView {
        view::library_panel(self.host, shown)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library_core::testkit::{link, row_at, shelf};

    fn blob() -> LibraryBlob {
        LibraryBlob {
            books: vec![
                row_at("a", "/lib/zeta.pdf"),
                row_at("b", "/lib/Alpha Notes.md"),
                row_at("c", "/lib/deep/plain.txt"),
                row_at("d", "/lib/deep/Book.pdf"),
                link("l", "Shortcut", "d"),
                link("dead", "Gone", "nobody"),
            ],
            shelves: vec![
                shelf("s1", "Work", &["c", "l"], None),
                shelf("s2", "Archive", &["d"], Some("s1")),
                shelf("s3", "empty", &[], None),
            ],
            ..LibraryBlob::default()
        }
    }

    fn names(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| match line {
                Line::Folder { name, depth, .. } => format!("{depth}:[{name}]"),
                Line::File { file, depth } => format!("{depth}:{} {}", file.name, file.badge),
            })
            .collect()
    }

    #[test]
    fn the_root_lists_folders_first_then_unfiled_files_by_name() {
        let tree = LibraryTree::from_blob(&blob());
        let shown = lines(&tree, &HashSet::new());
        assert_eq!(
            names(&shown),
            ["0:[empty]", "0:[Work]", "0:Alpha Notes MD", "0:zeta PDF"]
        );
    }

    #[test]
    fn an_open_folder_shows_its_folders_and_files_one_level_deeper() {
        let tree = LibraryTree::from_blob(&blob());
        let open: HashSet<String> = ["s1", "s2"].iter().map(|s| s.to_string()).collect();
        let shown = lines(&tree, &open);
        assert_eq!(
            names(&shown),
            [
                "0:[empty]",
                "0:[Work]",
                "1:[Archive]",
                "2:Book PDF",
                "1:plain TXT",
                "1:Shortcut PDF",
                "0:Alpha Notes MD",
                "0:zeta PDF",
            ]
        );
        // The link opens the book it points at, under its own name.
        let Some(Line::File { file, .. }) = shown
            .iter()
            .find(|l| matches!(l, Line::File { file, .. } if file.key == "s1/l"))
        else {
            panic!("the link row");
        };
        assert_eq!(
            (file.book_id.as_str(), file.path.as_str()),
            ("d", "/lib/deep/Book.pdf")
        );
    }

    #[test]
    fn a_folder_counts_every_file_under_it() {
        let tree = LibraryTree::from_blob(&blob());
        let work = tree.root.folders.iter().find(|f| f.id == "s1").unwrap();
        assert_eq!(work.count(), 3);
    }

    #[test]
    fn a_shelf_cycle_is_unreachable_and_harmless() {
        let mut blob = blob();
        blob.shelves.push(shelf("x", "X", &[], Some("y")));
        blob.shelves.push(shelf("y", "Y", &[], Some("x")));
        let tree = LibraryTree::from_blob(&blob);
        assert_eq!(tree.root.folders.len(), 2);
    }

    #[test]
    fn beside_splits_right_then_down_when_right_has_no_room() {
        let bounds = |width, height| {
            Some(PaneBounds {
                x: 0.0,
                y: 0.0,
                width,
                height,
            })
        };
        assert_eq!(beside_axis(bounds(900.0, 800.0)), SplitAxis::Horizontal);
        assert_eq!(beside_axis(bounds(360.0, 800.0)), SplitAxis::Vertical);
        // No room either way: right, so the layout's refusal speaks.
        assert_eq!(beside_axis(bounds(360.0, 300.0)), SplitAxis::Horizontal);
        assert_eq!(beside_axis(None), SplitAxis::Horizontal);
        assert_eq!(beside_axis(bounds(0.0, 0.0)), SplitAxis::Horizontal);
    }

    #[test]
    fn badges_come_from_the_extension() {
        assert_eq!(badge_of("/a/b.pdf"), "PDF");
        assert_eq!(badge_of("C:\\x\\notes.markdown"), "MD");
        assert_eq!(badge_of("/x/readme.TXT"), "TXT");
        assert_eq!(badge_of("/x/.hidden"), "");
        assert_eq!(badge_of("/x/noext"), "");
        assert_eq!(badge_of("/x/a.epub3x"), "EPUB");
    }
}
