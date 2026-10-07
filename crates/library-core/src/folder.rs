//! A watched folder: the import options and the placement ledger.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use reader_core::format::Format;

use crate::book::{Book, Fingerprint};
use crate::scan::{FoundFile, admits, selectable_formats, subfolder_of};
use crate::shape::ShapeTree;
use crate::shelf::Shelf;
use crate::tracking::{Track, TrackingTree};

/// The sheet's opening size threshold: smaller is a stub, not a book.
const DEFAULT_MIN_SIZE: u64 = 30 * 1024;

/// The −/+ step, counted in steps so no caller invents a value.
const MIN_SIZE_STEP: u64 = 10 * 1024;

/// Bounds for the −/+ buttons; [`sanitize`] clamps back inside them.
pub const MIN_SIZE_FLOOR: u64 = 0;
pub const MIN_SIZE_CEIL: u64 = 500 * 1024;

/// How one folder is scanned; every field is honoured on each rescan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpts {
    /// Always a subset of [`selectable_formats`].
    #[serde(default = "default_formats")]
    pub formats: BTreeSet<Format>,
    /// `true` = only these; `false` = all but these. One set and a flip.
    #[serde(default = "default_true")]
    pub include_selected: bool,
    /// Strict lower bound in bytes: a file of exactly this size is refused.
    #[serde(default = "default_min_size")]
    pub min_size: u64,
    /// Link each book to its address, or copy it in. Defaults to link.
    #[serde(default = "default_true")]
    pub in_place: bool,
    /// Legacy mirror of the root rung's tracking answer.
    #[serde(default)]
    pub watch: bool,
    /// Cut a shelf per subfolder (`true`) or keep the whole tree on one shelf.
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
    /// Step the threshold by one press, clamped to its bounds.
    pub fn step_min_size(&mut self, delta: i32) {
        let steps = delta as i64;
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next.clamp(MIN_SIZE_FLOOR as i64, MIN_SIZE_CEIL as i64) as u64;
    }

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

    /// The mode the two switches add up to.
    pub fn mode(&self) -> FolderMode {
        FolderMode::from_opts(self)
    }
}

/// The ways an import holds its books, computed from the two switches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderMode {
    /// Copy every admitted file into the library's own store.
    Copy,
    LinkInPlace,
    /// Read each book in place and re-walk on open or focus.
    LinkInPlaceWatched,
}

impl FolderMode {
    /// A match over the pair: a watching copy is a copy.
    fn from_opts(opts: &FolderOpts) -> Self {
        match (opts.in_place, opts.watch) {
            (false, _) => FolderMode::Copy,
            (true, false) => FolderMode::LinkInPlace,
            (true, true) => FolderMode::LinkInPlaceWatched,
        }
    }

    pub fn copies_files(self) -> bool {
        matches!(self, FolderMode::Copy)
    }

    pub fn reads_in_place(self) -> bool {
        !self.copies_files()
    }

    /// The root's answer only; which rungs are actually tracked is the tree's
    /// question ([`WatchedFolder::tracked`], [`WatchedFolder::tracks_rung`]).
    pub fn tracks_new_files(self) -> bool {
        matches!(self, FolderMode::LinkInPlaceWatched)
    }

    /// The mode's one user-facing wording; a mode described two ways reads as
    /// two modes.
    pub fn label(self) -> &'static str {
        match self {
            FolderMode::Copy => "Copy into the library",
            FolderMode::LinkInPlace => "Read at place",
            FolderMode::LinkInPlaceWatched => "Read at place, and watch for new books",
        }
    }

    /// Two-word badge form: where the books are.
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
    /// Never rewritten: a moved folder is a missing folder.
    pub root: String,
    #[serde(default)]
    pub opts: FolderOpts,
    /// Fingerprints already placed; a rescan skips a book the reader moved.
    #[serde(default)]
    pub placed: HashSet<Fingerprint>,
    /// Books the reader removed, one [`Tombstone`] each, so a rescan does
    /// not re-add them.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest scan saw: fingerprint to address.
    #[serde(default)]
    pub last_seen: Vec<(Fingerprint, String)>,
    /// Rung key to shelf id, persisted so a rescan reuses the shelf.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, String>,
    /// `0` until the first scan; diagnostic only.
    #[serde(default)]
    pub scanned_ms: u64,
    /// Per-rung tracking answers. [`crate::tracking::TrackingTree`] owns the
    /// inheritance; [`sanitize`] carries the legacy [`FolderOpts::watch`] flag
    /// into it.
    #[serde(default)]
    pub tracking: TrackingTree,
    /// Per-rung shelf-shape answers. [`ShapeTree`] owns the inheritance;
    /// [`WatchedFolder::set_shape`] keeps the root's answer equal to
    /// [`FolderOpts::groups`].
    #[serde(default)]
    pub shapes: ShapeTree,
}

impl WatchedFolder {
    /// A row never walked: one constructor for the empty ledger.
    pub fn new(id: impl Into<String>, root: impl Into<String>, opts: FolderOpts) -> Self {
        Self {
            id: id.into(),
            root: root.into(),
            opts,
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::new(),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
            shapes: ShapeTree::default(),
        }
    }
}

/// `path` relative to `root`, no edge separators; a directory edge,
/// not a string prefix.
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

/// Every rung of a key's path, root first, the key last.
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

/// The rung a key sits inside; the root is inside nothing.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    match key.rfind('/') {
        Some(at) => Some(&key[..at]),
        None => Some(""),
    }
}

/// Whether a key stands inside a zone; the empty zone is the tree.
pub fn key_in_zone(key: &str, zone: &str) -> bool {
    if zone.is_empty() {
        return true;
    }
    key == zone
        || key
            .strip_prefix(zone)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The address a rung stands at: [`rel_under`] run backwards.
pub fn dir_of_rung(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_string();
    }
    format!("{}/{}", root.trim_end_matches(['/', '\\']), rel)
}

impl WatchedFolder {
    /// The shelf shape at `key`: the deepest rung that answered.
    pub fn shape_at(&self, key: &str) -> bool {
        self.shapes.at(key).unwrap_or(self.opts.groups)
    }

    /// Record a rung's shape; the root's answer stands for the tree.
    pub fn set_shape(&mut self, rung: &str, grouped: bool) {
        if rung.is_empty() {
            self.opts.groups = grouped;
            self.shapes.prune_zone("");
        } else {
            self.shapes.set(rung, grouped);
        }
    }

    /// Whether the shape cuts a rung here: the root is always cut.
    pub fn cuts(&self, key: &str) -> bool {
        key.is_empty() || self.shape_at(key)
    }

    /// The rung an address's own folder answers for.
    pub fn rung_for(&self, key: &str) -> String {
        key_chain(key)
            .into_iter()
            .rev()
            .find(|rung| self.cuts(rung))
            .unwrap_or_default()
            .to_string()
    }

    /// The ledger key for a found file: [`Self::rung_for`] its folder.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        self.rung_for(found.subfolder())
    }

    /// The two shelves this tree names for an address: its rung's and the
    /// root.
    pub fn rungs_for(&self, path: &str) -> (Option<&str>, Option<&str>) {
        let Some(rel) = rel_under(path, &self.root) else {
            return (None, None);
        };
        let key = self.rung_for(subfolder_of(&rel));
        (
            self.shelf_map.get(key.as_str()).map(String::as_str),
            self.shelf_map.get("").map(String::as_str),
        )
    }

    /// The shelf a found file belongs on, minting the rungs up to it.
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

    /// Whether the whole tree is tracked from its root.
    pub fn tracked(&self) -> bool {
        self.tracking.tracked()
    }

    /// The folder's [`FolderMode`]: what a run does, and re-walks.
    pub fn mode(&self) -> FolderMode {
        self.opts.mode()
    }

    /// The rung's own tracking decision, or the nearest ancestor that has one.
    pub fn tracks_rung(&self, key: &str) -> bool {
        self.tracking.resolve(key)
    }

    /// Whether any rung is watched.
    fn tracks_anything(&self) -> bool {
        self.tracking.tracked() || self.tracking.any_on()
    }

    /// Whether a walk is owed: in place, with some rung on.
    pub fn owes_walk(&self) -> bool {
        self.mode().reads_in_place() && self.tracks_anything()
    }

    /// Turn tracking on for one rung, mirroring the root onto the flag.
    pub fn set_tracking(&mut self, key: &str, on: bool) {
        self.tracking
            .set(key, if on { Track::On } else { Track::Off });
        self.opts.watch = self.tracking.tracked();
    }

    /// Turn the whole tree on or off: every rung's override goes with it.
    pub fn set_tracking_whole(&mut self, on: bool) {
        self.tracking.set_root(on);
        self.opts.watch = on;
    }

    /// Drop map pointers at shelves that no longer stand.
    pub fn prune_shelf_map(&mut self, shelves: &[Shelf]) -> bool {
        let before = self.shelf_map.len();
        self.shelf_map
            .retain(|_, id| shelves.iter().any(|s| &s.id == id));
        self.shelf_map.len() != before
    }

    /// Whether a removal stands against `fp`.
    pub fn is_ignored(&self, fp: &Fingerprint) -> bool {
        self.ignored.iter().any(|entry| &entry.fp == fp)
    }

    /// Remember what this scan saw, for placed fingerprints.
    pub fn record_seen(&mut self, found: &[FoundFile]) {
        let seen: Vec<(Fingerprint, String)> = found
            .iter()
            .filter(|file| self.placed.contains(&file.fp))
            .map(|file| (file.fp, file.path.clone()))
            .collect();
        self.last_seen = seen;
    }
}

/// A removal, remembered by the folder that placed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    pub fp: Fingerprint,
    /// `None` for a book never opened; the menu uses the stem.
    #[serde(default)]
    pub title: Option<String>,
    pub format: Format,
    /// A restore re-measures this address first.
    pub last_path: String,
    /// A restore returns the book here, or to the root shelf.
    #[serde(default)]
    pub shelf_id: Option<String>,
    #[serde(default)]
    pub removed_ms: u64,
    /// The removal was a move, not a deletion.
    #[serde(default)]
    pub moved: bool,
    /// The row that represents this file, when one came home.
    #[serde(default)]
    pub returned_row: Option<String>,
}

impl Tombstone {
    /// The first of the folder's shelves the book was on.
    pub fn of(book: &Book, shelf_id: Option<String>, now_ms: u64) -> Self {
        Self {
            fp: book.fp,
            // The name the shelf showed, not the title field.
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

/// Lookup by id; a folder that is gone answers `None`.
pub fn find<'a>(folders: &'a [WatchedFolder], id: &str) -> Option<&'a WatchedFolder> {
    folders.iter().find(|f| f.id == id)
}

pub fn find_mut<'a>(folders: &'a mut [WatchedFolder], id: &str) -> Option<&'a mut WatchedFolder> {
    folders.iter_mut().find(|f| f.id == id)
}

/// Drop id-less and root-less rows, dedupe by root, clamp the threshold.
pub fn sanitize(folders: &mut Vec<WatchedFolder>) {
    let mut seen = HashSet::new();
    folders.retain(|f| {
        !f.id.trim().is_empty() && !f.root.trim().is_empty() && seen.insert(f.root.clone())
    });
    for f in folders.iter_mut() {
        f.opts.min_size = f.opts.min_size.clamp(MIN_SIZE_FLOOR, MIN_SIZE_CEIL);
        if f.opts.formats.is_empty() {
            f.opts.formats = default_formats();
        }
        // Carry the legacy flag into the tree; keep it equal to the root.
        if f.tracking.is_empty() && f.opts.watch {
            f.tracking = TrackingTree::tracking_root();
        }
        f.opts.watch = f.tracking.tracked();
        f.shelf_map
            .retain(|k, v| !v.trim().is_empty() && !k.contains('\\'));
        // One tombstone per fingerprint.
        let mut stones = HashSet::new();
        f.ignored
            .retain(|t| !t.last_path.trim().is_empty() && stones.insert(t.fp));
        let mut seen = HashSet::new();
        f.last_seen
            .retain(|(fp, path)| !path.trim().is_empty() && seen.insert(*fp));
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
        assert_eq!(
            rel_under("/books/a/b.pdf", "/books").as_deref(),
            Some("a/b.pdf")
        );
        assert_eq!(
            rel_under("/books/b.pdf", "/books").as_deref(),
            Some("b.pdf")
        );
        assert_eq!(rel_under("/books", "/books").as_deref(), Some(""));
        assert_eq!(rel_under("/books/", "/books/").as_deref(), Some(""));
        assert_eq!(rel_under("/bookshelf/a.pdf", "/book"), None);
        assert_eq!(rel_under("/other/a.pdf", "/books"), None);
        // Windows paths answer in `/`; a `\` key never matches again.
        assert_eq!(
            rel_under("C:\\books\\a\\b.pdf", "C:\\books").as_deref(),
            Some("a/b.pdf")
        );
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
        // A drag off the tree's shelf for this file is the departure rule.
        assert_eq!(
            f.rungs_for("/books/Fiction/other.pdf"),
            (Some("shelf2"), Some("shelf1"))
        );
        assert_eq!(
            f.rungs_for("/books/top.pdf"),
            (Some("shelf1"), Some("shelf1"))
        );
        assert_eq!(f.rungs_for("/books/Unmapped/x.pdf"), (None, Some("shelf1")));
        assert_eq!(f.rungs_for("/other/x.pdf"), (None, None));
    }

    #[test]
    fn a_shape_answered_for_one_rung_cuts_the_rungs_under_it_alone() {
        let mut f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            shelf_map: BTreeMap::from([
                (String::new(), "root".to_string()),
                ("Fiction".to_string(), "fic".to_string()),
            ]),
            ..folder("/books")
        };
        assert_eq!(
            f.rung_for("Fiction/SciFi"),
            "",
            "one shelf files every address on its root rung"
        );
        assert_eq!(f.rungs_for("/books/Reference/x.pdf").0, Some("root"));
        // The re-imported folder cuts its own rungs; above keeps the rest.
        f.set_shape("Fiction", true);
        assert_eq!(f.rung_for("Fiction"), "Fiction");
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        assert_eq!(
            f.rung_for("Reference"),
            "",
            "the rest of the tree is where it was"
        );
        assert_eq!(
            f.rungs_for("/books/Fiction/SciFi/dune.pdf").0,
            None,
            "no shelf stands for the rung the answer cut yet"
        );
        assert_eq!(f.rungs_for("/books/Fiction/other.pdf").0, Some("fic"));
    }

    #[test]
    fn the_roots_answer_stands_for_the_whole_tree() {
        let mut f = WatchedFolder {
            opts: FolderOpts {
                groups: false,
                ..FolderOpts::default()
            },
            ..folder("/books")
        };
        f.set_shape("Fiction", true);
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        f.set_shape("", true);
        assert!(f.opts.groups, "the root's answer IS the folder's own shape");
        assert!(
            f.shapes.is_empty(),
            "and it takes every deeper answer with it"
        );
        assert_eq!(f.rung_for("Fiction/SciFi"), "Fiction/SciFi");
        f.set_shape("", false);
        assert_eq!(
            f.rung_for("Fiction/SciFi"),
            "",
            "one shelf puts every directory on the root rung"
        );
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
        assert_eq!(
            f.rungs_for("/books/Fiction/SciFi/dune.pdf"),
            (Some("root"), Some("root"))
        );
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("root"), Some("root")));
    }

    #[test]
    fn the_rung_a_walk_names_and_the_rung_an_address_names_agree() {
        // One key arithmetic: rescan and drag cannot disagree.
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
            shapes: crate::shape::ShapeTree::default(),
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
        let f: WatchedFolder = serde_json::from_str(r#"{"id":"f1","root":"/books"}"#).unwrap();
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
        let o = FolderOpts {
            min_size: 512,
            ..Default::default()
        };
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
        let at_root = FoundFile {
            rel: "dune.pdf".into(),
            ..found
        };
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
            made.iter()
                .map(|(rung, _, _, _)| rung.as_str())
                .collect::<Vec<_>>(),
            vec!["", "scifi", "scifi/deep"]
        );
        assert_eq!(made[2].2, "deep", "the leaf is named by its own subfolder");
        assert_eq!(
            made[0].3, None,
            "the folder's own shelf hangs at the level it was on"
        );
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
        // Carry the legacy flag into the tree, or a load stops rescanning.
        let raw = r#"{"id":"f1","root":"/books","opts":{"inPlace":true,"watch":true}}"#;
        let mut folders: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw}]")).unwrap();
        assert!(folders[0].tracking.is_empty(), "the old blob has no tree");
        assert!(folders[0].opts.watch, "and the flag it did have");
        sanitize(&mut folders);
        assert!(
            folders[0].tracked(),
            "the flag became the root rung's decision"
        );
        assert!(
            folders[0].tracks_rung("Fiction"),
            "and the tree below it inherits"
        );
        assert!(
            folders[0].opts.watch,
            "the flag is left agreed with the tree"
        );
        let raw_off = r#"{"id":"f2","root":"/dvds","opts":{"inPlace":true,"watch":false}}"#;
        let mut off: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw_off}]")).unwrap();
        sanitize(&mut off);
        assert!(!off[0].tracked());
        // The tree is the answer from here on; a stale flag yields to it.
        let mut written = vec![mode("f3", "/books", true, false)];
        written[0].set_tracking("", true);
        assert!(
            written[0].opts.watch,
            "set_tracking mirrors the root onto the flag"
        );
        written[0].opts.watch = false;
        sanitize(&mut written);
        assert!(written[0].tracked(), "the tree wins");
        assert!(
            written[0].opts.watch,
            "and the flag is brought back into agreement"
        );
    }

    #[test]
    fn turning_a_rung_off_below_a_watched_root_leaves_the_root_watching() {
        // The flag could not express this: root on, rung below off.
        let mut f = folder("/books");
        f.set_tracking("", true);
        assert!(f.tracked() && f.tracks_rung("Fiction") && f.tracks_rung("Fiction/SciFi"));
        f.set_tracking("Fiction", false);
        assert!(f.tracked(), "the tree is still watched");
        assert!(f.opts.watch, "and the flag still says so");
        assert!(!f.tracks_rung("Fiction"), "the rung turned off is off");
        assert!(
            !f.tracks_rung("Fiction/SciFi"),
            "and so is everything below it"
        );
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
        // A hand-edited blob must not kill a watched folder.
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
        // A `\` in a key means it was never normalised.
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
        // A dead pointer is a rung the walk reuses.
        let standing = [crate::testkit::folder_shelf(
            "s1",
            "Books",
            "f1",
            None,
            &[],
            None,
        )];
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
        // Through the writer, not the field: `set_tracking` keeps the tree and
        // the flag agreed.
        find_mut(&mut folders, "f2").unwrap().set_tracking("", true);
        assert!(find(&folders, "f2").is_some_and(|f| f.tracked() && f.opts.watch));
    }

    #[test]
    fn the_two_switches_add_up_to_one_mode_and_its_questions() {
        // One switch combination is not offerable; it folds in `mode`.
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
        assert!(
            folder.opts.watch,
            "the root's answer is mirrored onto the flag"
        );
        assert_eq!(folder.mode(), FolderMode::LinkInPlaceWatched);
        assert_eq!(folder.opts.mode(), folder.mode());
    }

    #[test]
    fn a_folder_the_library_copies_is_owed_no_walk_whatever_its_tree_says() {
        // The unofferable pair folds into `Copy` at the mode.
        let copying = mode("f1", "/books", false, true);
        assert_eq!(
            copying.mode(),
            FolderMode::Copy,
            "a watching copy is a copy"
        );
        assert!(copying.tracks_anything(), "and its tree is still standing");
        assert!(!copying.owes_walk(), "so nothing walks it");

        // A root turned off with one subfolder left on is still watched.
        let mut partly = mode("f2", "/dvds", true, true);
        partly.set_tracking("", false);
        partly.set_tracking("Films", true);
        assert!(!partly.opts.watch, "the root's own answer is off");
        assert!(partly.owes_walk(), "and the subfolder still owes the walk");

        let quiet = mode("f3", "/comics", true, false);
        assert!(!quiet.owes_walk());
    }

    /// The watch arrives as a tree; the flag mirrors its root.
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
