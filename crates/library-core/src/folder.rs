//! A watched folder: the options an import was made with, and the ledger of what
//! that import already did.
//!
//! The ledger half is the reason this is a struct and not a settings row: "which
//! files did I already place" is not the same question as "which files are in the
//! library", and a rescan runs on every window focus.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use reader_core::format::Format;

use crate::book::{Book, Fingerprint};
use crate::scan::{FoundFile, admits, selectable_formats, subfolder_of};
use crate::shelf::Shelf;
use crate::tracking::{Track, TrackingTree};

/// The default size threshold the import sheet opens on, in bytes: a PDF smaller
/// than this is a stub, a placeholder or a corrupt download. A default, not a rule.
const DEFAULT_MIN_SIZE: u64 = 30 * 1024;

/// The step the sheet's −/+ buttons move the threshold by: a count of steps rather
/// than a byte delta, so no caller can invent a value the control could not have
/// produced.
const MIN_SIZE_STEP: u64 = 10 * 1024;

/// The bounds the sheet's −/+ buttons move between, and what [`sanitize`] brings a loaded blob back inside.
pub const MIN_SIZE_FLOOR: u64 = 0;
pub const MIN_SIZE_CEIL: u64 = 500 * 1024;

/// How one folder is scanned. Every field is a choice the import sheet offers, and
/// every one of them is honoured on EVERY later rescan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpts {
    /// Always a subset of [`selectable_formats`].
    #[serde(default = "default_formats")]
    pub formats: BTreeSet<Format>,
    /// `true` = only these formats; `false` = everything but these. One set and a flip rather than two lists that can contradict each other.
    #[serde(default = "default_true")]
    pub include_selected: bool,
    /// Strict lower bound in bytes: a file of exactly this size is refused.
    #[serde(default = "default_min_size")]
    pub min_size: u64,
    /// Read in place. `true` links each book to the address it was found at; `false`
    /// copies it into the app's store. Default `true`, so a reader who upgrades keeps
    /// the library they had.
    #[serde(default = "default_true")]
    pub in_place: bool,
    /// Rescan this folder when the app opens or regains focus. Only offered alongside
    /// [`FolderOpts::in_place`] in the sheet, but honoured independently here.
    #[serde(default)]
    pub watch: bool,
    /// Cut a shelf per subfolder, or keep the whole tree on one shelf.
    #[serde(default = "default_true")]
    pub groups: bool,
}

fn default_formats() -> BTreeSet<Format> {
    selectable_formats().into_iter().collect()
}

fn default_true() -> bool {
    true
}

fn default_min_size() -> u64 {
    DEFAULT_MIN_SIZE
}

impl Default for FolderOpts {
    fn default() -> Self {
        Self {
            formats: default_formats(),
            include_selected: true,
            min_size: DEFAULT_MIN_SIZE,
            in_place: true,
            watch: false,
            groups: true,
        }
    }
}

impl FolderOpts {
    /// The threshold stepped by one press of the sheet's −/+, clamped to the bounds the
    /// sheet shows.
    pub fn step_min_size(&mut self, delta: i32) {
        let steps = delta as i64;
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next
            .clamp(MIN_SIZE_FLOOR as i64, MIN_SIZE_CEIL as i64)
            as u64;
    }

    /// A size in bytes as the sheet prints it; whole KB drops the decimals so the control reads "30 KB".
    pub fn min_size_label(&self) -> String {
        if self.min_size.is_multiple_of(1024) {
            format!("{} KB", self.min_size / 1024)
        } else {
            format!("{:.1} KB", self.min_size as f64 / 1024.0)
        }
    }

    pub fn admits_file(&self, ext: &str, size: u64) -> bool {
        admits(self, ext, size)
    }

    /// The mode these two switches add up to. The switches are the WIRE; this is the answer every caller asks for.
    pub fn mode(&self) -> FolderMode {
        FolderMode::from_opts(self)
    }
}

/// The three ways a folder import can hold its books, read off the two switches
/// [`FolderOpts`] carries. A computed view over the pair rather than a fourth field
/// to migrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMode {
    /// Copy every admitted file into the library's own store.
    Copy,
    LinkInPlace,
    /// Read each book at the address it was found at, and walk the tree again when the app opens or regains focus.
    LinkInPlaceWatched,
}

impl FolderMode {
    /// Written as a `match` over the pair rather than two nested `if`s so the folding of
    /// the unofferable combination is visible in one place: a watching copy is a copy,
    /// because tracking is a promise about the tree the books are READ from.
    pub fn from_opts(opts: &FolderOpts) -> Self {
        match (opts.in_place, opts.watch) {
            (false, _) => FolderMode::Copy,
            (true, false) => FolderMode::LinkInPlace,
            (true, true) => FolderMode::LinkInPlaceWatched,
        }
    }

    pub fn copies_files(self) -> bool {
        matches!(self, FolderMode::Copy)
    }

    /// The complement of [`FolderMode::copies_files`], spelled out because it is the question the import, the departure and the conflict sheets read.
    pub fn reads_in_place(self) -> bool {
        !self.copies_files()
    }

    /// The ROOT's answer, read off [`FolderOpts::watch`]. Which rungs are actually
    /// tracked is the tree's own question ([`WatchedFolder::tracked`],
    /// [`WatchedFolder::tracks_rung`]).
    pub fn tracks_new_files(self) -> bool {
        matches!(self, FolderMode::LinkInPlaceWatched)
    }

    /// The mode's own name, in the words the app asks the question in; a mode described
    /// two ways reads as two modes.
    pub fn label(self) -> &'static str {
        match self {
            FolderMode::Copy => "Copy into the library",
            FolderMode::LinkInPlace => "Read at place",
            FolderMode::LinkInPlaceWatched => "Read at place, and watch for new books",
        }
    }

    /// [`FolderMode::label`] cut down to two words, for the badge a folder card wears.
    /// What it says is where the books are: the dot beside it is the watched signal.
    pub fn badge(self) -> &'static str {
        match self {
            FolderMode::Copy => "Copied",
            FolderMode::LinkInPlace | FolderMode::LinkInPlaceWatched => "On disk",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: String,
    /// Never rewritten by the app: a moved folder is a missing folder, and the sheet offers a new path rather than guessing.
    pub root: String,
    #[serde(default)]
    pub opts: FolderOpts,
    /// Fingerprints THIS folder has already placed. Membership is what makes a rescan
    /// honest: a book the reader moved keeps its fingerprint here, so the next scan
    /// skips it instead of putting it back.
    #[serde(default)]
    pub placed: HashSet<Fingerprint>,
    /// The books the reader deliberately removed. A tombstone per removal: the file is
    /// still on disk and still admitted by `opts`, so without one the next rescan would
    /// re-add exactly what was just deleted. The folder's import menu reads it too.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest scan saw, restricted to the fingerprints this folder has placed:
    /// fingerprint to the address it was found at. The import menu's other half, which
    /// is what lets it open instantly instead of walking the tree again.
    #[serde(default)]
    pub last_seen: Vec<(Fingerprint, String)>,
    /// Persisted so a rescan adds to the shelf the last one created rather than making a second shelf of the same name.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, String>,
    /// `0` until the first scan completes. Diagnostic only: no decision reads it, so a stale stamp can never suppress a scan.
    #[serde(default)]
    pub scanned_ms: u64,
    /// Which rungs of this tree are tracked, per rung rather than for the whole import.
    /// [`crate::tracking::TrackingTree`] owns the inheritance, and [`sanitize`] carries
    /// the legacy [`FolderOpts::watch`] flag across into it.
    #[serde(default)]
    pub tracking: TrackingTree,
}

/// The path of `path` relative to `root`, `/`-separated, with no leading or trailing
/// separator. `Some("")` when the two name the same directory, `None` when `path` is
/// not inside `root` at all — a directory edge rather than a string prefix, which is
/// what keeps "/bookshelf" out of "/book".
pub fn rel_under(path: &str, root: &str) -> Option<String> {
    fn norm(p: &str) -> String {
        p.trim_end_matches(['/', '\\']).replace('\\', "/")
    }
    let (path, root) = (norm(path), norm(root));
    if path == root {
        return Some(String::new());
    }
    let rest = path.strip_prefix(root.as_str())?.strip_prefix('/')?;
    (!rest.is_empty()).then(|| rest.to_string())
}

/// Every rung of a shelf key's path, root first and the key itself last: `""`, `"2"`, `"2/deep"`.
pub fn key_chain(key: &str) -> Vec<&str> {
    let mut out = vec![""];
    if key.is_empty() {
        return out;
    }
    for (at, _) in key.match_indices('/') {
        out.push(&key[..at]);
    }
    out.push(key);
    out
}

/// The rung a shelf key sits inside: `"2/deep"` is inside `"2"`, and the root is inside nothing.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    match key.rfind('/') {
        Some(at) => Some(&key[..at]),
        None => Some(""),
    }
}

/// Whether a rung key stands inside a zone of the tree: the zone itself, or anywhere
/// below it. The empty zone is the whole tree. A directory edge rather than a string
/// prefix, the rule [`rel_under`] gives for addresses.
pub fn key_in_zone(key: &str, zone: &str) -> bool {
    if zone.is_empty() {
        return true;
    }
    key == zone
        || key
            .strip_prefix(zone)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The address a rung's directory stands at: [`rel_under`] run backwards. One
/// function owns each direction so a shelf's ground and its folder's map cannot
/// drift about what the key names.
pub fn dir_of_rung(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_string();
    }
    format!("{}/{}", root.trim_end_matches(['/', '\\']), rel)
}

impl WatchedFolder {
    /// The ledger key for a found file: its subfolder when the folder groups, the empty
    /// string otherwise. One function owns the choice so the walk, the shelf creation
    /// and the persisted map cannot disagree about it.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        if self.opts.groups {
            found.subfolder().to_string()
        } else {
            String::new()
        }
    }

    /// The two rungs this folder's tree names for an address: the shelf the address's own
    /// subfolder maps to, and the shelf at the folder's root. Both are `None` when the
    /// address is not under this folder at all.
    ///
    /// Two answers because the two questions asked of them differ: *where does this file
    /// come back to* wants the root as a fallback, while *has this file left its ground*
    /// wants the first answer alone — treating the root as a second home would let a book
    /// be dragged rung to rung while still answering to the folder that placed it.
    pub fn rungs_for(&self, path: &str) -> (Option<&str>, Option<&str>) {
        let Some(rel) = rel_under(path, &self.root) else {
            return (None, None);
        };
        let key = if self.opts.groups {
            subfolder_of(&rel)
        } else {
            ""
        };
        (
            self.shelf_map.get(key).map(String::as_str),
            self.shelf_map.get("").map(String::as_str),
        )
    }

    /// The shelf a found file belongs on, minting EVERY rung between the folder's root
    /// shelf and the file's own subfolder, and reporting each rung it mints through `made`.
    ///
    /// A walk reports files, not directories, so minting only the leaf would hang a
    /// subfolder's shelf off the root with a hole above it. Rungs already in
    /// [`WatchedFolder::shelf_map`] are reused rather than re-minted, which is what makes
    /// a rescan continue the tree instead of growing a twin beside it.
    pub fn shelf_chain_for(
        &mut self,
        key: &str,
        mut mint: impl FnMut(&str) -> String,
        mut name_of: impl FnMut(&str) -> String,
        mut made: impl FnMut(&str, &str, String, Option<String>),
    ) -> String {
        let mut current: Option<String> = None;
        let mut id = String::new();
        for rung in key_chain(key) {
            id = match self.shelf_map.get(rung) {
                Some(known) => known.clone(),
                None => {
                    let fresh = mint(rung);
                    self.shelf_map.insert(rung.to_string(), fresh.clone());
                    made(rung, &fresh, name_of(rung), current.clone());
                    fresh
                }
            };
            current = Some(id.clone());
        }
        id
    }

    /// Record that this folder placed a file, so the next rescan skips it.
    pub fn mark_placed(&mut self, fp: Fingerprint) {
        self.placed.insert(fp);
    }

    /// Whether this folder's tree is tracked from its root — the whole-tree answer the
    /// single [`FolderOpts::watch`] flag used to be. Every surface that draws a watch dot ask
    /// this rather than the flag.
    pub fn tracked(&self) -> bool {
        self.tracking.tracked()
    }

    /// What a run on this folder does with the files it finds, and whether a later one
    /// walks the tree again: the one answer the services and surfaces read instead of
    /// testing the two switches apart.
    pub fn mode(&self) -> FolderMode {
        self.opts.mode()
    }

    /// The rung's own decision, or the nearest ancestor that has one.
    pub fn tracks_rung(&self, key: &str) -> bool {
        self.tracking.resolve(key)
    }

    /// Whether this folder is watched ANYWHERE: its root, or any rung the reader turned on
    /// under a root they turned off. A tree only a subfolder of is watched still owes the
    /// walk.
    pub fn tracks_anything(&self) -> bool {
        self.tracking.tracked() || self.tracking.any_on()
    }

    /// Whether a walk is owed: whether there is a watch at all — tracking is a promise
    /// about the folder the books are READ from, and a folder the library copies has none
    /// to keep — and whether any rung of its tree is on.
    pub fn owes_walk(&self) -> bool {
        self.mode().reads_in_place() && self.tracks_anything()
    }

    /// Turn tracking on or off for one rung, and mirror the root's answer back onto
    /// [`FolderOpts::watch`] so the import sheet's switch agrees with the tree. The flag is
    /// the legacy half of one decision rather than a second source of truth.
    pub fn set_tracking(&mut self, key: &str, on: bool) {
        self.tracking.set(key, if on { Track::On } else { Track::Off });
        self.opts.watch = self.tracking.tracked();
    }

    /// Turn the WHOLE tree on or off: every rung's override goes with it, because "the
    /// whole folder" is a sentence about every rung in it.
    pub fn set_tracking_whole(&mut self, on: bool) {
        self.tracking.set_root(on);
        self.opts.watch = on;
    }

    /// Drop the map's pointers at shelves that are no longer standing, and answer whether
    /// it dropped any. A dead pointer is a rung the walk REUSES instead of minting, and the
    /// placement that rides it lands on no shelf at all.
    pub fn prune_shelf_map(&mut self, shelves: &[Shelf]) -> bool {
        let before = self.shelf_map.len();
        self.shelf_map
            .retain(|_, id| shelves.iter().any(|s| &s.id == id));
        self.shelf_map.len() != before
    }

    /// Whether this folder is holding a removal against `fp`. A tombstone wins over everything: the reader said no.
    pub fn is_ignored(&self, fp: &Fingerprint) -> bool {
        self.ignored.iter().any(|entry| &entry.fp == fp)
    }

    /// Remember what this scan saw, for the fingerprints this folder placed. Written on
    /// every scan, including one that changed nothing: the menu's "moved out of this
    /// folder" answer is only as fresh as the last walk.
    pub fn record_seen(&mut self, found: &[FoundFile]) {
        let seen: Vec<(Fingerprint, String)> = found
            .iter()
            .filter(|file| self.placed.contains(&file.fp))
            .map(|file| (file.fp, file.path.clone()))
            .collect();
        self.last_seen = seen;
    }
}

/// A book the reader removed, remembered by the folder that placed it. It keeps the
/// file out of every later rescan, and it is the record the folder's import menu reads
/// to offer the book back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub fp: Fingerprint,
    /// `None` for a book that was imported and never opened; the menu falls back to the file's stem.
    #[serde(default)]
    pub title: Option<String>,
    pub format: Format,
    /// A restore re-measures this address first: a file that has since moved is a relink, not a restore.
    pub last_path: String,
    /// A restore puts it back there if the shelf still exists, and on the folder's root shelf if it does not.
    #[serde(default)]
    pub shelf_id: Option<String>,
    #[serde(default)]
    pub removed_ms: u64,
    /// True when the removal was a MOVE and not a deletion: the library still holds the
    /// book, so the restore menu must not offer it back as a book that is gone.
    #[serde(default)]
    pub moved: bool,
    /// The row that represents this file to the folder, when one came home: a later import
    /// of the file lights that row up instead of minting a linked neighbour beside it.
    #[serde(default)]
    pub returned_row: Option<String>,
}

impl Tombstone {
    /// `shelf_id` is the first of the folder's shelves the book was on, if any: a book on three of them still comes back to one.
    pub fn of(book: &Book, shelf_id: Option<String>, now_ms: u64) -> Self {
        Self {
            fp: book.fp,
            // The name the SHELF showed (`Book::title`), not the raw title field: a stored book the
            // reader never opened has no title of its own, and the log would otherwise label itself
            // from the store's `source.pdf`.
            title: Some(book.title()),
            format: book.format,
            last_path: book.path().to_string(),
            shelf_id,
            removed_ms: now_ms,
            moved: false,
            returned_row: None,
        }
    }

    pub fn label(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), &self.last_path)
    }
}

/// Every question about a folder starts from an id the library already holds, and every
/// caller spelled the same walk to answer it — so a folder that is gone answers `None`
/// everywhere, rather than a silent no-match.
pub fn find<'a>(folders: &'a [WatchedFolder], id: &str) -> Option<&'a WatchedFolder> {
    folders.iter().find(|f| f.id == id)
}

pub fn find_mut<'a>(folders: &'a mut [WatchedFolder], id: &str) -> Option<&'a mut WatchedFolder> {
    folders.iter_mut().find(|f| f.id == id)
}

/// Drop rows with no id or no root, dedupe by root (first wins), clamp the size
/// threshold, and empty a format set that would admit nothing. Idempotent.
pub fn sanitize(folders: &mut Vec<WatchedFolder>) {
    let mut seen = HashSet::new();
    folders.retain(|f| {
        !f.id.trim().is_empty() && !f.root.trim().is_empty() && seen.insert(f.root.clone())
    });
    for f in folders.iter_mut() {
        f.opts.min_size = f
            .opts
            .min_size
            .clamp(MIN_SIZE_FLOOR, MIN_SIZE_CEIL);
        if f.opts.formats.is_empty() {
            f.opts.formats = default_formats();
        }
        // A watch on a folder that is not read in place still means something (the source may
        // gain a file worth copying), so it is left alone. The legacy flag is carried into the
        // rung tree the first time this build sees the folder, and kept equal to the tree's
        // root so the two cannot drift.
        if f.tracking.is_empty() && f.opts.watch {
            f.tracking.set("", Track::On);
        }
        f.opts.watch = f.tracking.tracked();
        f.shelf_map.retain(|k, v| !v.trim().is_empty() && !k.contains('\\'));
        // One tombstone per fingerprint: a book removed twice must not leave two rows offering the same file back.
        let mut stones = HashSet::new();
        f.ignored.retain(|t| !t.last_path.trim().is_empty() && stones.insert(t.fp));
        let mut seen = HashSet::new();
        f.last_seen.retain(|(fp, path)| !path.trim().is_empty() && seen.insert(*fp));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_key_is_a_chain_of_rungs_root_first() {
        assert_eq!(key_chain(""), vec![""]);
        assert_eq!(key_chain("2"), vec!["", "2"]);
        assert_eq!(key_chain("2/deep"), vec!["", "2", "2/deep"]);
        assert_eq!(parent_key(""), None);
        assert_eq!(parent_key("2"), Some(""));
        assert_eq!(parent_key("2/deep"), Some("2"));
    }

    #[test]
    fn inside_a_folder_starts_on_a_directory_edge() {
        assert_eq!(rel_under("/books/a/b.pdf", "/books").as_deref(), Some("a/b.pdf"));
        assert_eq!(rel_under("/books/b.pdf", "/books").as_deref(), Some("b.pdf"));
        assert_eq!(rel_under("/books", "/books").as_deref(), Some(""));
        assert_eq!(rel_under("/books/", "/books/").as_deref(), Some(""));
        assert_eq!(rel_under("/bookshelf/a.pdf", "/book"), None);
        assert_eq!(rel_under("/other/a.pdf", "/books"), None);
        // Windows answers in `/` like every other path in the ledger: a key holding a `\` never matches a found file again.
        assert_eq!(rel_under("C:\\books\\a\\b.pdf", "C:\\books").as_deref(), Some("a/b.pdf"));
    }

    #[test]
    fn a_zone_holds_its_own_key_and_the_rungs_below_it() {
        assert!(key_in_zone("2", "2"));
        assert!(key_in_zone("2/deep", "2"));
        assert!(key_in_zone("2/deep/deeper", "2"));
        assert!(!key_in_zone("20", "2"));
        assert!(!key_in_zone("20/deep", "2"));
        assert!(!key_in_zone("3", "2"));
        assert!(!key_in_zone("", "2"));
        assert!(key_in_zone("", ""));
        assert!(key_in_zone("2", ""));
        assert!(key_in_zone("2/deep", ""));
    }

    #[test]
    fn a_rung_s_address_is_its_key_joined_back_onto_the_root() {
        assert_eq!(dir_of_rung("/books", ""), "/books");
        assert_eq!(dir_of_rung("/books", "2/3"), "/books/2/3");
        assert_eq!(dir_of_rung("/books/", "2"), "/books/2");
        assert_eq!(dir_of_rung("C:\\books", "2"), "C:\\books/2");
        let dir = dir_of_rung("/books", "2/3");
        assert_eq!(rel_under(&dir, "/books").as_deref(), Some("2/3"));
    }

    #[test]
    fn a_file_stands_on_the_rung_its_own_subfolder_names() {
        let f = WatchedFolder {
            shelf_map: BTreeMap::from([
                ("".to_string(), "shelf1".to_string()),
                ("Fiction".to_string(), "shelf2".to_string()),
                ("Fiction/SciFi".to_string(), "shelf3".to_string()),
            ]),
            ..folder("/books")
        };
        let deep = "/books/Fiction/SciFi/dune.pdf";
        assert_eq!(f.rungs_for(deep), (Some("shelf3"), Some("shelf1")));
        // A drag from shelf3 to shelf2 has left the ground the folder's tree names for this
        // file, which is the whole of the departure rule.
        assert_eq!(
            f.rungs_for("/books/Fiction/other.pdf"),
            (Some("shelf2"), Some("shelf1"))
        );
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("shelf1"), Some("shelf1")));
        assert_eq!(f.rungs_for("/books/Unmapped/x.pdf"), (None, Some("shelf1")));
        assert_eq!(f.rungs_for("/other/x.pdf"), (None, None));
    }

    #[test]
    fn a_folder_that_does_not_group_has_one_rung_for_every_file() {
        let f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            shelf_map: BTreeMap::from([
                ("".to_string(), "root".to_string()),
                ("Fiction".to_string(), "ignored".to_string()),
            ]),
            ..folder("/books")
        };
        assert_eq!(f.rungs_for("/books/Fiction/SciFi/dune.pdf"), (Some("root"), Some("root")));
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("root"), Some("root")));
    }

    #[test]
    fn the_rung_a_walk_names_and_the_rung_an_address_names_agree() {
        // One key arithmetic behind both is the reason a rescan and a drag cannot disagree about where a file belongs.
        let f = WatchedFolder {
            shelf_map: BTreeMap::from([("Fiction/SciFi".to_string(), "shelf3".to_string())]),
            ..folder("/books")
        };
        let found = FoundFile {
            path: "/books/Fiction/SciFi/dune.pdf".into(),
            rel: "Fiction/SciFi/dune.pdf".into(),
            ext: "pdf".into(),
            size: 1,
            fp: fp(1),
        };
        assert_eq!(f.shelf_key(&found), "Fiction/SciFi");
        assert_eq!(f.rungs_for(&found.path).0, Some("shelf3"));
    }

    fn fp(n: u32) -> Fingerprint {
        Fingerprint {
            size: u64::from(n),
            mtime_ms: u64::from(n),
            head_hash: n,
        }
    }

    fn folder(root: &str) -> WatchedFolder {
        WatchedFolder {
            id: "f1".into(),
            root: root.into(),
            opts: FolderOpts::default(),
            placed: HashSet::new(),
            ignored: Vec::new(),
            shelf_map: BTreeMap::new(),
            last_seen: Vec::new(),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
        }
    }

    fn stone(n: u32) -> Tombstone {
        Tombstone {
            fp: fp(n),
            title: Some(format!("Book {n}")),
            format: Format::Pdf,
            last_path: format!("/books/{n}.pdf"),
            shelf_id: None,
            removed_ms: 5,
            moved: false,
            returned_row: None,
        }
    }

    #[test]
    fn the_defaults_are_the_ones_the_sheet_opens_on() {
        let o = FolderOpts::default();
        assert_eq!(o.min_size, DEFAULT_MIN_SIZE);
        assert_eq!(o.min_size_label(), "30 KB");
        assert!(o.include_selected);
        assert!(o.in_place, "read in place is the mode the app always had");
        assert!(!o.watch, "watching is opt-in");
        assert!(o.groups);
        assert_eq!(o.formats.len(), selectable_formats().len());
    }

    #[test]
    fn a_blob_from_before_the_folder_options_existed_loads_them() {
        let f: WatchedFolder =
            serde_json::from_str(r#"{"id":"f1","root":"/books"}"#).unwrap();
        assert_eq!(f.opts, FolderOpts::default());
        assert!(f.placed.is_empty() && f.ignored.is_empty());
        assert!(f.shelf_map.is_empty());
    }

    #[test]
    fn the_size_dial_steps_in_kb_and_stops_at_its_bounds() {
        let mut o = FolderOpts::default();
        o.step_min_size(1);
        assert_eq!(o.min_size, 40 * 1024);
        o.step_min_size(-2);
        assert_eq!(o.min_size, 20 * 1024);
        for _ in 0..100 {
            o.step_min_size(-1);
        }
        assert_eq!(o.min_size, MIN_SIZE_FLOOR);
        assert_eq!(o.min_size_label(), "0 KB");
        for _ in 0..200 {
            o.step_min_size(1);
        }
        assert_eq!(o.min_size, MIN_SIZE_CEIL);
        assert_eq!(o.min_size_label(), "500 KB");
    }

    #[test]
    fn a_sub_thousand_byte_threshold_still_prints_honestly() {
        let mut o = FolderOpts::default();
        o.min_size = 512;
        assert_eq!(o.min_size_label(), "0.5 KB");
    }

    #[test]
    fn the_ledger_skips_what_it_placed_and_honours_a_tombstone() {
        let mut f = folder("/books");
        assert!(!f.is_ignored(&fp(1)));
        f.mark_placed(fp(1));
        assert!(f.placed.contains(&fp(1)));
        assert!(!f.is_ignored(&fp(1)));
        f.ignored.push(stone(1));
        assert!(f.is_ignored(&fp(1)), "a removal outranks everything");
    }

    #[test]
    fn grouping_decides_whether_a_subfolder_is_its_own_shelf() {
        let mut f = folder("/books");
        let found = FoundFile {
            path: "/books/scifi/dune.pdf".into(),
            rel: "scifi/dune.pdf".into(),
            ext: "pdf".into(),
            size: 1,
            fp: fp(1),
        };
        assert_eq!(f.shelf_key(&found), "scifi");
        f.opts.groups = false;
        assert_eq!(f.shelf_key(&found), "", "one flat shelf for the whole tree");
        f.opts.groups = true;
        let at_root = FoundFile { rel: "dune.pdf".into(), ..found };
        assert_eq!(f.shelf_key(&at_root), "");
    }

    #[test]
    fn the_shelf_map_reuses_the_shelf_it_minted() {
        let mut f = folder("/books");
        let mut made: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut seq = 0usize;
        let leaf = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.rsplit('/').next().unwrap_or(rung).to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(
            made.iter().map(|(rung, _, _, _)| rung.as_str()).collect::<Vec<_>>(),
            vec!["", "scifi", "scifi/deep"]
        );
        assert_eq!(made[2].2, "deep", "the leaf is named by its own subfolder");
        assert_eq!(made[0].3, None, "the folder's own shelf hangs at the level it was on");
        assert_eq!(made[1].3.as_deref(), Some(made[0].1.as_str()));
        assert_eq!(made[2].3.as_deref(), Some(made[1].1.as_str()));
        assert_eq!(leaf, made[2].1);
        assert_eq!(f.shelf_map.len(), 3);

        made.clear();
        let again = f.shelf_chain_for(
            "scifi/deep",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(again, leaf);
        assert!(made.is_empty(), "no rung was minted, so none was reported");

        let sibling = f.shelf_chain_for(
            "scifi/deep/er",
            |_| {
                seq += 1;
                format!("s{seq}")
            },
            |rung| rung.to_string(),
            |rung, id, name, parent| made.push((rung.to_string(), id.to_string(), name, parent)),
        );
        assert_eq!(made.len(), 1, "only the new leaf");
        assert_eq!(made[0].3.as_deref(), Some(leaf.as_str()));
        assert_ne!(sibling, leaf);
    }

    #[test]
    fn a_blob_from_before_tracking_was_a_tree_keeps_watching() {
        // A folder written by an older build carries `opts.watch` and no tree, and an empty
        // tree tracks nothing — so a load that did not carry the flag across would silently
        // stop rescanning every watched folder the reader had.
        let raw = r#"{"id":"f1","root":"/books","opts":{"inPlace":true,"watch":true}}"#;
        let mut folders: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw}]")).unwrap();
        assert!(folders[0].tracking.is_empty(), "the old blob has no tree");
        assert!(folders[0].opts.watch, "and the flag it did have");
        sanitize(&mut folders);
        assert!(folders[0].tracked(), "the flag became the root rung's decision");
        assert!(folders[0].tracks_rung("Fiction"), "and the tree below it inherits");
        assert!(folders[0].opts.watch, "the flag is left agreed with the tree");
        let raw_off = r#"{"id":"f2","root":"/dvds","opts":{"inPlace":true,"watch":false}}"#;
        let mut off: Vec<WatchedFolder> =
            serde_json::from_str(&format!("[{raw_off}]")).unwrap();
        sanitize(&mut off);
        assert!(!off[0].tracked());
        // A stale flag yields to the tree: the tree is the answer from here on.
        let mut written = vec![mode("f3", "/books", true, false)];
        written[0].set_tracking("", true);
        assert!(written[0].opts.watch, "set_tracking mirrors the root onto the flag");
        written[0].opts.watch = false;
        sanitize(&mut written);
        assert!(written[0].tracked(), "the tree wins");
        assert!(written[0].opts.watch, "and the flag is brought back into agreement");
    }

    #[test]
    fn turning_a_rung_off_below_a_watched_root_leaves_the_root_watching() {
        // The control the single flag could not express: the root keeps its answer, the rung below does not.
        let mut f = folder("/books");
        f.set_tracking("", true);
        assert!(f.tracked() && f.tracks_rung("Fiction") && f.tracks_rung("Fiction/SciFi"));
        f.set_tracking("Fiction", false);
        assert!(f.tracked(), "the tree is still watched");
        assert!(f.opts.watch, "and the flag still says so");
        assert!(!f.tracks_rung("Fiction"), "the rung turned off is off");
        assert!(!f.tracks_rung("Fiction/SciFi"), "and so is everything below it");
        assert!(f.tracks_rung("Poetry"), "a sibling is untouched");
        f.tracking.set("Fiction", crate::tracking::Track::Inherit);
        assert!(f.tracks_rung("Fiction/SciFi"));
    }

    #[test]
    fn sanitize_dedupes_roots_and_clamps_the_dial() {
        let mut folders = vec![
            WatchedFolder {
                opts: FolderOpts {
                    min_size: 10_000_000,
                    ..FolderOpts::default()
                },
                ..folder("/books")
            },
            folder("/books"),
            folder(""),
            WatchedFolder {
                id: " ".into(),
                ..folder("/other")
            },
        ];
        sanitize(&mut folders);
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].opts.min_size, MIN_SIZE_CEIL);
    }

    #[test]
    fn an_empty_format_set_is_not_a_folder_that_admits_nothing() {
        // A hand-edited blob, or a format removed from the registry, must not silently turn a watched folder into a dead one.
        let mut folders = vec![WatchedFolder {
            opts: FolderOpts {
                formats: BTreeSet::new(),
                ..FolderOpts::default()
            },
            ..folder("/books")
        }];
        sanitize(&mut folders);
        assert_eq!(folders[0].opts.formats.len(), selectable_formats().len());
    }

    #[test]
    fn a_backslash_never_survives_into_the_shelf_map() {
        // A `\` in a key means the blob was written by something that did not normalise it, and that key would never match a found file again.
        let mut folders = vec![WatchedFolder {
            shelf_map: BTreeMap::from([
                ("scifi".to_string(), "s1".to_string()),
                ("scifi\\deep".to_string(), "s2".to_string()),
            ]),
            ..folder("/books")
        }];
        sanitize(&mut folders);
        let keys: Vec<&String> = folders[0].shelf_map.keys().collect();
        assert_eq!(keys, vec!["scifi"]);
    }

    #[test]
    fn a_map_pointer_at_a_shelf_that_went_is_cut() {
        // A dead pointer is a rung the walk REUSES instead of minting, and every placement that
        // rides it lands on no shelf at all — a run that counts a book it filed nowhere.
        let standing = [crate::testkit::folder_shelf("s1", "Books", "f1", None, &[], None)];
        let mut f = folder("/books");
        f.shelf_map = BTreeMap::from([
            (String::new(), "s1".to_string()),
            ("scifi".to_string(), "gone".to_string()),
        ]);
        assert!(f.prune_shelf_map(&standing), "a dead pointer is news");
        let keys: Vec<&String> = f.shelf_map.keys().collect();
        assert_eq!(keys, vec![""], "and the rung that stands is kept");
        assert!(
            !f.prune_shelf_map(&standing),
            "cutting it once is the whole of it"
        );
        let none: [Shelf; 0] = [];
        assert!(f.prune_shelf_map(&none));
        assert!(f.shelf_map.is_empty());
    }

    #[test]
    fn a_folder_is_found_by_id_for_a_read_and_for_a_write() {
        let mut folders = vec![folder("/one"), folder("/two")];
        folders[1].id = "f2".into();
        assert_eq!(find(&folders, "f2").map(|f| f.root.as_str()), Some("/two"));
        assert!(find(&folders, "gone").is_none());
        // Through the writer rather than the field: `set_tracking` is what keeps the tree and the flag agreed.
        find_mut(&mut folders, "f2").unwrap().set_tracking("", true);
        assert!(find(&folders, "f2").is_some_and(|f| f.tracked() && f.opts.watch));
    }

    #[test]
    fn the_two_switches_add_up_to_one_mode_and_its_questions() {
        // The sheet offers three combinations and one of them is not offerable, so the folding happens where the mode is computed.
        let opts = |in_place: bool, watch: bool| FolderOpts {
            in_place,
            watch,
            ..FolderOpts::default()
        };
        assert_eq!(opts(true, false).mode(), FolderMode::LinkInPlace);
        assert_eq!(opts(true, true).mode(), FolderMode::LinkInPlaceWatched);
        assert_eq!(opts(false, false).mode(), FolderMode::Copy);
        assert_eq!(
            opts(false, true).mode(),
            FolderMode::Copy,
            "a copy does not care what the source folder does next"
        );
        assert!(opts(false, true).mode().copies_files());
        assert!(!opts(false, true).mode().reads_in_place());
        assert!(opts(true, true).mode().reads_in_place());
        assert!(!opts(true, true).mode().copies_files());
        assert!(opts(true, true).mode().tracks_new_files());
        assert!(!opts(true, false).mode().tracks_new_files());
        let mut folder = folder("/books");
        folder.opts.in_place = true;
        folder.set_tracking("", true);
        assert!(folder.opts.watch, "the root's answer is mirrored onto the flag");
        assert_eq!(folder.mode(), FolderMode::LinkInPlaceWatched);
        assert_eq!(folder.opts.mode(), folder.mode());
    }

    #[test]
    fn a_folder_the_library_copies_is_owed_no_walk_whatever_its_tree_says() {
        // The pair the sheet cannot offer — `in_place=false, watch=true` — folds into `Copy`, so
        // a tree left standing on a copying folder is chrome rather than a second opinion.
        let copying = mode("f1", "/books", false, true);
        assert_eq!(copying.mode(), FolderMode::Copy, "a watching copy is a copy");
        assert!(copying.tracks_anything(), "and its tree is still standing");
        assert!(!copying.owes_walk(), "so nothing walks it");

        // A root turned off with one subfolder left on reads as an unwatched folder and is not one.
        let mut partly = mode("f2", "/dvds", true, true);
        partly.set_tracking("", false);
        partly.set_tracking("Films", true);
        assert!(!partly.opts.watch, "the root's own answer is off");
        assert!(partly.owes_walk(), "and the subfolder still owes the walk");

        let quiet = mode("f3", "/comics", true, false);
        assert!(!quiet.owes_walk());
    }

    /// The watch arrives as a TREE rather than as the flag alone, because the flag is now the
    /// root rung's mirror: a fixture that set only the flag would be a folder this build
    /// never writes.
    fn mode(id: &str, root: &str, in_place: bool, watch: bool) -> WatchedFolder {
        let mut folder = WatchedFolder {
            id: id.into(),
            opts: FolderOpts {
                in_place,
                watch,
                ..FolderOpts::default()
            },
            ..folder(root)
        };
        if watch {
            folder.set_tracking("", true);
        }
        folder
    }
}
