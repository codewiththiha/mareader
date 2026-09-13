//! A watched folder: the options an import was made with, and the ledger of
//! what that import already did.
//!
//! The ledger half is the reason this is a struct and not a settings row. A
//! rescan runs on every window focus, and "which files did I already place"
//! is not the same question as "which files are in the library": a book the
//! reader dragged off a folder shelf is still in the library, and re-adding it
//! would undo the arrangement; a book the reader deliberately removed is not
//! in the library, and re-adding it would ignore the removal. Both answers
//! live here, per folder, and [`crate::ledger`] reads them.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use serde::{Deserialize, Serialize};

use reader_core::format::Format;

use crate::book::{Book, Fingerprint};
use crate::scan::{FoundFile, admits, selectable_formats, subfolder_of};
use crate::shelf::Shelf;
use crate::tracking::{Track, TrackingTree};

/// The default size threshold the import sheet opens on, in bytes: 30 KB. A
/// PDF smaller than that is a stub, a placeholder or a corrupt download, and a
/// text file smaller than that is rarely a book — but the number is a default,
/// not a rule, and the sheet shows it in KB because that is how a reader
/// thinks about it.
const DEFAULT_MIN_SIZE: u64 = 30 * 1024;

/// The step the sheet's −/+ buttons move the threshold by. Crate-private, and
/// not because it is a secret: [`FolderOpts::step_min_size`] takes a count of
/// steps rather than a byte delta precisely so no caller can invent a value the
/// control could not have produced, and publishing the step would invite one.
const MIN_SIZE_STEP: u64 = 10 * 1024;

/// The bounds the sheet's −/+ buttons move between. Public because they are the
/// sheet's: the stepper disables itself at them, and [`sanitize`] brings a loaded
/// blob back inside them, so the two have to be the same two numbers.
pub const MIN_SIZE_FLOOR: u64 = 0;
pub const MIN_SIZE_CEIL: u64 = 500 * 1024;

/// How one folder is scanned. Every field is a choice the import sheet offers,
/// and every one of them is honoured on EVERY later rescan — editing an option
/// after the fact changes what the next scan admits, and never removes a book
/// the previous scan already placed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderOpts {
    /// The formats the selection names. Always a subset of
    /// [`selectable_formats`]; the sheet offers exactly those.
    #[serde(default = "default_formats")]
    pub formats: BTreeSet<Format>,
    /// `true` = only these formats; `false` = everything but these. One set
    /// and a flip rather than two lists that can contradict each other.
    #[serde(default = "default_true")]
    pub include_selected: bool,
    /// Strict lower bound in bytes: a file of exactly this size is refused.
    #[serde(default = "default_min_size")]
    pub min_size: u64,
    /// Read in place. `true` links each book to the address it was found at;
    /// `false` copies it into the app's store. Default `true`, because linking
    /// is the only thing this app ever did and the copy is the new behaviour —
    /// a reader who upgrades keeps the library they had.
    #[serde(default = "default_true")]
    pub in_place: bool,
    /// Rescan this folder when the app opens or regains focus. Only offered
    /// alongside [`FolderOpts::in_place`] in the sheet (a store copy does not
    /// care what the source folder does next), but honoured independently
    /// here: the ledger's job is the same either way.
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
    /// The threshold stepped by one press of the sheet's −/+, clamped to the
    /// bounds the sheet shows. `delta` is in steps, not bytes, so the caller
    /// cannot invent a value the control could not have produced.
    pub fn step_min_size(&mut self, delta: i32) {
        let steps = delta as i64;
        let next = self.min_size as i64 + steps * MIN_SIZE_STEP as i64;
        self.min_size = next
            .clamp(MIN_SIZE_FLOOR as i64, MIN_SIZE_CEIL as i64)
            as u64;
    }

    /// A size in bytes, as the sheet prints it. Whole KB drops the decimals so
    /// the control reads "30 KB" and not "30.0 KB".
    pub fn min_size_label(&self) -> String {
        if self.min_size.is_multiple_of(1024) {
            format!("{} KB", self.min_size / 1024)
        } else {
            format!("{:.1} KB", self.min_size as f64 / 1024.0)
        }
    }

    /// [`admits`] bound to these options, for the walk that filters as it goes.
    pub fn admits_file(&self, ext: &str, size: u64) -> bool {
        admits(self, ext, size)
    }
}

/// One folder the library watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchedFolder {
    pub id: String,
    /// The absolute path the walk starts at. Never rewritten by the app: a
    /// moved folder is a missing folder, and the sheet offers a new path
    /// rather than guessing.
    pub root: String,
    #[serde(default)]
    pub opts: FolderOpts,
    /// Fingerprints THIS folder has already placed. Membership is what makes a
    /// rescan honest: a book the reader moved to another shelf, or off the
    /// folder's shelf entirely, keeps its fingerprint here, so the next scan
    /// skips it instead of putting it back.
    #[serde(default)]
    pub placed: HashSet<Fingerprint>,
    /// The books the reader deliberately removed from the library. A tombstone
    /// per removal: the file is still on disk and still admitted by `opts`, so
    /// without one the next rescan would re-add exactly what was just deleted.
    /// Only written for books this folder placed.
    ///
    /// A list of records rather than a set of fingerprints, because a tombstone
    /// has a second job: it is what the folder's import menu reads to offer the
    /// book back. A set could say "not this one again" and nothing more.
    #[serde(default)]
    pub ignored: Vec<Tombstone>,
    /// What the latest scan saw, restricted to the fingerprints this folder has
    /// placed: fingerprint to the address it was found at.
    ///
    /// The import menu's other half. "Did a book I filed here move somewhere
    /// else?" is answerable from this and the library's membership lists alone,
    /// which is what lets the menu open instantly instead of walking the tree
    /// again — and the restriction to placed fingerprints is what bounds it: a
    /// folder cannot have placed more books than the library holds.
    #[serde(default)]
    pub last_seen: Vec<(Fingerprint, String)>,
    /// Subfolder (relative, `/`-separated, `""` for the root) to the shelf its
    /// books were placed on. Persisted so a rescan adds to the shelf the last
    /// one created rather than making a second shelf with the same name.
    #[serde(default)]
    pub shelf_map: BTreeMap<String, String>,
    /// When this folder was last rescanned, in milliseconds since the epoch.
    /// `0` until the first scan completes. Diagnostic only — no decision reads
    /// it, which is why a stale stamp can never suppress a scan.
    #[serde(default)]
    pub scanned_ms: u64,
    /// Which rungs of this tree are tracked, per rung rather than for the whole
    /// import. [`WatchedFolder::tracked`] is the answer every caller reads;
    /// [`crate::tracking::TrackingTree`] owns the inheritance.
    ///
    /// `#[serde(default)]` because a blob written before tracking was a tree has
    /// no key at all, and [`sanitize`] is what carries the legacy
    /// [`FolderOpts::watch`] flag across into it — an empty tree tracks nothing,
    /// so a load that skipped that step would silently stop rescanning every
    /// watched folder the reader had.
    #[serde(default)]
    pub tracking: TrackingTree,
}

/// The path of `path` relative to `root`: `/`-separated on every platform, with
/// no leading or trailing separator, so `"/books/a/b.pdf"` under `"/books"` is
/// `"a/b.pdf"`. `Some("")` when the two name the same directory, and `None` when
/// `path` is not inside `root` at all — a directory edge rather than a string
/// prefix, which is what keeps `"/bookshelf"` out of `"/book"`.
///
/// Every question the library asks about a watched folder's ground starts here:
/// which folders cover an address, which shelf a directory is, and which rung a
/// file stands on. One function owns the arithmetic so those answers cannot
/// disagree about what "inside" means.
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

/// Every rung of a shelf key's path, root first and the key itself last: `""`,
/// then `"2"`, then `"2/deep"`. The root rung is always first because the watched
/// folder's own shelf is the top of every chain it mints.
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

/// The rung a shelf key sits inside: `"2/deep"` is inside `"2"`, `"2"` is inside
/// the root, and the root is inside nothing. What a rescan re-hangs a folder
/// shelf's `parent` from.
pub fn parent_key(key: &str) -> Option<&str> {
    if key.is_empty() {
        return None;
    }
    match key.rfind('/') {
        Some(at) => Some(&key[..at]),
        None => Some(""),
    }
}

/// Whether a rung key stands inside a zone of the tree: the zone itself, or
/// anywhere below it. The empty zone is the whole tree, because the watched
/// root is every rung's ancestor — a departure of the root rung takes every
/// key the folder's map holds with it.
///
/// A directory edge rather than a string prefix, the rule [`rel_under`] gives
/// for addresses: `"2"` is inside `"2"` and inside nothing else that starts
/// with those characters, so a zone of `"2"` does not swallow `"20/deep"`.
/// What a shelf's departure asks of the folder's map — which rung keys leave
/// the tree with it — is the same question [`rel_under`] asks of a file, and
/// the two answer with one arithmetic.
pub fn key_in_zone(key: &str, zone: &str) -> bool {
    if zone.is_empty() {
        return true;
    }
    key == zone
        || key
            .strip_prefix(zone)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The address a rung's directory stands at: the watched root for the root
/// rung, and the root with the rung's key joined onto it for any rung below.
///
/// [`rel_under`] run backwards. One function owns each direction so a shelf's
/// ground and its folder's map cannot drift about what the key names: the
/// family questions — which tree a folder belongs to, which tree a rung is a
/// member of — all start by turning the rung back into the address the disk
/// knows, and hand it to [`rel_under`] like every other ground question.
pub fn dir_of_rung(root: &str, rel: &str) -> String {
    if rel.is_empty() {
        return root.to_string();
    }
    format!("{}/{}", root.trim_end_matches(['/', '\\']), rel)
}

impl WatchedFolder {
    /// The ledger key for a found file: its subfolder when the folder groups,
    /// the empty string for the root otherwise. One function owns the choice so
    /// the walk, the shelf creation and the persisted map cannot disagree about
    /// it.
    ///
    /// Owned rather than borrowed because the caller goes straight from this to
    /// [`WatchedFolder::shelf_chain_for`], which takes `&mut self`: a key borrowed
    /// from the folder would still be alive when the folder is mutated.
    pub fn shelf_key(&self, found: &FoundFile) -> String {
        if self.opts.groups {
            found.subfolder().to_string()
        } else {
            String::new()
        }
    }

    /// The two rungs this folder's tree names for an address: the shelf the
    /// address's own subfolder maps to, and the shelf at the folder's root. Both
    /// are `None` when the address is not under this folder at all, and the first
    /// is `None` on its own when the subfolder has no shelf of its own — an
    /// address the walk has never placed, or one whose rung the reader deleted.
    ///
    /// Two answers rather than one because the two questions asked of them are
    /// different, and both need the ledger's own arithmetic:
    ///
    ///   * *where does this file come back to* wants the chain — the subfolder's
    ///     rung, the root's when it has none. That is the fallback
    ///     [`crate::folder::WatchedFolder::shelf_chain_for`] walks down when it
    ///     mints, so a restore lands where the folder itself would have put it;
    ///   * *has this file left its ground* wants the first answer alone, with no
    ///     fallback. A book sitting on the root rung of a folder that groups is
    ///     not on the ground its own subfolder names, and treating the root as a
    ///     second home would let a book be dragged rung to rung down the tree
    ///     while still answering to the folder that placed it.
    ///
    /// [`WatchedFolder::shelf_key`]'s arithmetic from an address instead of a
    /// walk's finding, because a book already in the library has no
    /// [`FoundFile`]. Same key, same `groups` rule, same map: the walk and the
    /// membership edit cannot disagree about which rung a file stands on.
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

    /// The shelf a found file belongs on, minting EVERY rung between the folder's
    /// root shelf and the file's own subfolder, and reporting each rung it mints
    /// through `made` (rung, id, name, the parent id above it) so the caller can
    /// put a shelf row under the id.
    ///
    /// A walk reports files, not directories: minting only the leaf would hang a
    /// subfolder's shelf off the root with a hole above it, and a library that
    /// showed the tree flat beside the tree nested was two logics wearing one
    /// shelf list. The chain is minted level by level instead, so an intermediate
    /// directory with no books of its own is an empty folder card rather than a
    /// missing rung, and the shelf at every depth is the directory at that depth.
    ///
    /// Rungs already in [`WatchedFolder::shelf_map`] are reused rather than
    /// re-minted, which is what makes a rescan continue the tree instead of
    /// growing a twin beside it. Called from the frontend, which owns the shelf
    /// list and the id sequence; the shell's walk only reports what it found.
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

    /// Whether this folder's tree is tracked from its root — the whole-tree
    /// answer the single [`FolderOpts::watch`] flag used to be, now read off the
    /// rung tree so a subfolder can say something different from its root.
    ///
    /// Every surface that draws a watch dot, and the rescan that decides which
    /// folders to walk, ask this rather than the flag: one rule for "is this
    /// folder watched", and a rung-level answer available to the surfaces that
    /// want one ([`WatchedFolder::tracks_rung`]).
    pub fn tracked(&self) -> bool {
        self.tracking.tracked()
    }

    /// Whether one rung of this tree is tracked: the rung's own decision, or the
    /// nearest ancestor that has one. The root rung is [`WatchedFolder::tracked`].
    pub fn tracks_rung(&self, key: &str) -> bool {
        self.tracking.resolve(key)
    }

    /// Whether this folder is watched ANYWHERE: its root, or any rung the
    /// reader turned on under a root they turned off. The walk question —
    /// which folders a focus rescan owes a pass — where [`WatchedFolder::tracked`]
    /// is the root's own answer: a tree only a subfolder of is watched still
    /// owes the walk, and the ledger's per-rung gate is what keeps the quiet
    /// rungs quiet inside it.
    pub fn tracks_anything(&self) -> bool {
        self.tracking.tracked() || self.tracking.any_on()
    }

    /// Turn tracking on or off for one rung of this tree, and mirror the root's
    /// answer back onto [`FolderOpts::watch`] so the import sheet's switch and
    /// every surface that reads the flag agree with the tree.
    ///
    /// The flag is the legacy half of one decision rather than a second source of
    /// truth: a blob written by an older build carries the flag and no tree, and
    /// [`sanitize`] carries it across on load. Writing both keeps a downgrade
    /// honest in the one direction that matters — a folder this build watched is a
    /// folder an older build still watches.
    pub fn set_tracking(&mut self, key: &str, on: bool) {
        self.tracking.set(key, if on { Track::On } else { Track::Off });
        self.opts.watch = self.tracking.tracked();
    }

    /// Drop the map's pointers at shelves that are no longer standing, and
    /// answer whether it dropped any.
    ///
    /// A dead pointer is a rung the walk REUSES instead of minting: the chain
    /// hands back an id nothing wears, the placement that rides it lands on no
    /// shelf, and the run counts a book it filed nowhere — a card that says a
    /// book came home and a shelf it is not on. The two writers that take a
    /// shelf off the list both cut the pointer themselves (a shelf taken apart,
    /// a rung's departure letting its zone go), so this is the repair for a map
    /// an older build or a hand left behind, and it is the map's own
    /// [`sanitize`](crate::folder::sanitize) with the shelf list in hand — the
    /// one thing that function cannot ask, because it is given one list.
    pub fn prune_shelf_map(&mut self, shelves: &[Shelf]) -> bool {
        let before = self.shelf_map.len();
        self.shelf_map
            .retain(|_, id| shelves.iter().any(|s| &s.id == id));
        self.shelf_map.len() != before
    }

    /// Whether this folder is holding a removal against `fp` — the question a
    /// rescan asks before any other. A tombstone wins over everything: the
    /// reader said no, and the file being unchanged since is not a new
    /// argument.
    pub fn is_ignored(&self, fp: &Fingerprint) -> bool {
        self.ignored.iter().any(|entry| &entry.fp == fp)
    }

    /// Remember what this scan saw, for the fingerprints this folder placed.
    ///
    /// Written on every scan, including one that changed nothing: the menu's
    /// "moved out of this folder" answer is only as fresh as the last walk, and a
    /// walk that found nothing to do is still a walk that saw every file.
    pub fn record_seen(&mut self, found: &[FoundFile]) {
        let seen: Vec<(Fingerprint, String)> = found
            .iter()
            .filter(|file| self.placed.contains(&file.fp))
            .map(|file| (file.fp, file.path.clone()))
            .collect();
        self.last_seen = seen;
    }
}

/// A book the reader removed from the library, remembered by the folder that
/// placed it.
///
/// Two jobs, and the second is why it carries more than a fingerprint: it keeps
/// the file out of every later rescan, and it is the record the folder's import
/// menu reads to offer the book back — with a name, a size and an address to
/// re-measure before it promises anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tombstone {
    /// The fingerprint the book carried when it was removed. What a rescan
    /// matches against, and what a restore looks up by.
    pub fp: Fingerprint,
    /// The name the shelf showed. `None` for a book that was imported and never
    /// opened, which is most of them; the menu falls back to the file's stem.
    #[serde(default)]
    pub title: Option<String>,
    pub format: Format,
    /// Where the file lived when it was removed. A restore re-measures this
    /// address first: a file that has since moved is a relink, not a restore, and
    /// the menu says so rather than importing a path that is not there.
    pub last_path: String,
    /// The shelf it was filed on, when it was filed on one. A restore puts it
    /// back there if the shelf still exists, and on the folder's root shelf if it
    /// does not — a removed book should not come back somewhere new.
    #[serde(default)]
    pub shelf_id: Option<String>,
    /// When it was removed, in milliseconds since the epoch.
    #[serde(default)]
    pub removed_ms: u64,
    /// True when the removal was a MOVE and not a deletion: the book left its
    /// folder as the library's own stored copy, so the library still holds it
    /// and the folder's restore menu must not offer it back as a book that is
    /// gone. The file on disk is untouched — this log is about the shelf, not
    /// about the filesystem.
    #[serde(default)]
    pub moved: bool,
    /// The row that represents this file to the folder, when one came home: a
    /// stored copy moved back onto a shelf this folder owns under the name the
    /// log remembers binds itself here, and a later import of the file lights
    /// that row up instead of minting a linked neighbour beside it. `None`
    /// until a return binds it, and stale ids are checked against the library
    /// before they are trusted.
    #[serde(default)]
    pub returned_row: Option<String>,
}

impl Tombstone {
    /// The record for a book about to be removed. `shelf_id` is the first of the
    /// folder's shelves the book was on, if any — one answer, deterministically
    /// chosen, because a book on three of a folder's shelves still comes back to
    /// one.
    pub fn of(book: &Book, shelf_id: Option<String>, now_ms: u64) -> Self {
        Self {
            fp: book.fp,
            // The name the SHELF showed (`Book::title`), not the raw title
            // field: a stored book the reader never opened has no title of its
            // own, and a log that kept `None` would label itself from
            // `last_path` — the store's `source.pdf` for a copy, which is the
            // layout's word for the book rather than the reader's.
            title: Some(book.title()),
            format: book.format,
            last_path: book.path().to_string(),
            shelf_id,
            removed_ms: now_ms,
            moved: false,
            returned_row: None,
        }
    }

    /// What a restore row calls the book: its own title, else the file's stem.
    pub fn label(&self) -> String {
        crate::text::display_or_stem(self.title.as_deref(), &self.last_path)
    }
}

/// The watched folder with this id.
///
/// Every question about a folder starts from an id the library already holds —
/// a shelf's [`ShelfKind::Folder`], a tombstone's owner, a dock card's run — and
/// every one of them was a walk of the list spelled at the call site. One walk
/// here means a folder that is gone answers the same way everywhere: `None`,
/// and the caller's "no such folder" branch rather than a silent no-op.
///
/// [`ShelfKind::Folder`]: crate::shelf::ShelfKind::Folder
pub fn find<'a>(folders: &'a [WatchedFolder], id: &str) -> Option<&'a WatchedFolder> {
    folders.iter().find(|f| f.id == id)
}

/// The same, for a write — the ledger's half, where a placement is recorded, a
/// tombstone is lifted and a shelf map is cut.
pub fn find_mut<'a>(folders: &'a mut [WatchedFolder], id: &str) -> Option<&'a mut WatchedFolder> {
    folders.iter_mut().find(|f| f.id == id)
}

/// Make a persisted folder list internally valid: drop rows with no id or no
/// root, dedupe by root (first wins), clamp the size threshold into the range
/// the sheet can produce, and empty a format set that would admit nothing.
/// Idempotent.
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
        // A watch on a folder that is not read in place still means something
        // (the source may gain a file worth copying), so it is left alone; the
        // sheet simply does not offer it there.
        //
        // The legacy flag is carried into the rung tree the first time this build
        // sees the folder, and the flag is then kept equal to the tree's root so
        // the two cannot drift. An empty tree is what a blob from before tracking
        // existed deserialises to, and an empty tree tracks nothing — so without
        // this step every watched folder a reader had would silently stop being
        // rescanned on the upgrade.
        if f.tracking.is_empty() && f.opts.watch {
            f.tracking.set("", Track::On);
        }
        f.opts.watch = f.tracking.tracked();
        f.shelf_map.retain(|k, v| !v.trim().is_empty() && !k.contains('\\'));
        // One tombstone per fingerprint: a book removed twice (it can happen —
        // restore it, then remove it again) must not leave two rows offering the
        // same file back, and the newest is the one that knows where it last was.
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
        // A string prefix is not a directory: `/bookshelf` is not in `/book`.
        assert_eq!(rel_under("/bookshelf/a.pdf", "/book"), None);
        assert_eq!(rel_under("/other/a.pdf", "/books"), None);
        // Windows answers in `/` like every other path in the ledger, because a
        // `shelf_map` key holding a `\` never matches a found file again.
        assert_eq!(rel_under("C:\\books\\a\\b.pdf", "C:\\books").as_deref(), Some("a/b.pdf"));
    }

    #[test]
    fn a_zone_holds_its_own_key_and_the_rungs_below_it() {
        assert!(key_in_zone("2", "2"));
        assert!(key_in_zone("2/deep", "2"));
        assert!(key_in_zone("2/deep/deeper", "2"));
        // A string prefix is not a directory: `20` is not inside `2`.
        assert!(!key_in_zone("20", "2"));
        assert!(!key_in_zone("20/deep", "2"));
        // A sibling and a parent are outside the zone.
        assert!(!key_in_zone("3", "2"));
        assert!(!key_in_zone("", "2"));
        // The root zone is the whole tree, including the root's own key.
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
        // And the two directions of the arithmetic agree, which is the point
        // of owning both: an address round-trips through its key.
        let dir = dir_of_rung("/books", "2/3");
        assert_eq!(rel_under(&dir, "/books").as_deref(), Some("2/3"));
    }

    #[test]
    fn a_file_stands_on_the_rung_its_own_subfolder_names() {
        // The three-level shelf the bug was reported against: `/books` cut into
        // `Fiction` cut into `Fiction/SciFi`, with the book at the bottom.
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
        // The rung above is a DIFFERENT rung: a drag from shelf3 to shelf2 has
        // left the ground the folder's tree names for this file, which is the
        // whole of the departure rule. Answering shelf2 here — or answering the
        // root as a second home — is what let a moved book keep wearing the
        // address and the next import of it highlight the row already dragged away.
        assert_eq!(
            f.rungs_for("/books/Fiction/other.pdf"),
            (Some("shelf2"), Some("shelf1"))
        );
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("shelf1"), Some("shelf1")));
        // A subfolder the walk never placed, and an address outside the folder:
        // no rung of its own either way, and the second answer says which.
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
        // Everything lands flat, so the root rung is the ground for every
        // address under it — and a stale deeper key in the map is not a rung
        // anybody stands on.
        assert_eq!(f.rungs_for("/books/Fiction/SciFi/dune.pdf"), (Some("root"), Some("root")));
        assert_eq!(f.rungs_for("/books/top.pdf"), (Some("root"), Some("root")));
    }

    #[test]
    fn the_rung_a_walk_names_and_the_rung_an_address_names_agree() {
        // `shelf_key` runs on the walk's finding, `rungs_for` on an address in
        // the library. One key arithmetic behind both is the reason a rescan and
        // a drag cannot disagree about where a file belongs; this is the tie.
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
        // Placing is not a tombstone: the book is on a shelf, so a rescan
        // skips it through `placed`, and removing it later still has to stick.
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
        // A file at the root of a grouped folder is on the folder's own shelf.
        f.opts.groups = true;
        let at_root = FoundFile { rel: "dune.pdf".into(), ..found };
        assert_eq!(f.shelf_key(&at_root), "");
    }

    #[test]
    fn the_shelf_map_reuses_the_shelf_it_minted() {
        let mut f = folder("/books");
        let mut made: Vec<(String, String, String, Option<String>)> = Vec::new();
        let mut seq = 0usize;
        // A file two subfolders deep mints the WHOLE chain — the folder's root
        // shelf, the rung under it, and the leaf — reporting each rung with the
        // parent above it, so an intermediate directory with no books of its
        // own is an empty folder card rather than a missing rung.
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

        // The second file reuses every rung it shares with the first: a rescan
        // never mints a second shelf for a subfolder, and mints only the rung
        // that is genuinely new.
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
        // The whole upgrade in one assertion: a folder written by an older build
        // carries `opts.watch` and no tree, and an empty tree tracks nothing — so
        // a load that did not carry the flag across would silently stop rescanning
        // every watched folder the reader had.
        let raw = r#"{"id":"f1","root":"/books","opts":{"inPlace":true,"watch":true}}"#;
        let mut folders: Vec<WatchedFolder> = serde_json::from_str(&format!("[{raw}]")).unwrap();
        assert!(folders[0].tracking.is_empty(), "the old blob has no tree");
        assert!(folders[0].opts.watch, "and the flag it did have");
        sanitize(&mut folders);
        assert!(folders[0].tracked(), "the flag became the root rung's decision");
        assert!(folders[0].tracks_rung("Fiction"), "and the tree below it inherits");
        assert!(folders[0].opts.watch, "the flag is left agreed with the tree");
        // The other direction: an unwatched folder stays unwatched, and a tree
        // that already has a decision is not overwritten by the flag beside it.
        let raw_off = r#"{"id":"f2","root":"/dvds","opts":{"inPlace":true,"watch":false}}"#;
        let mut off: Vec<WatchedFolder> =
            serde_json::from_str(&format!("[{raw_off}]")).unwrap();
        sanitize(&mut off);
        assert!(!off[0].tracked());
        // A folder this build wrote carries a tree, and a stale flag disagreeing
        // with it yields to the tree — the tree is the answer from here on.
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
        // The control the single flag could not express, asked of the folder
        // rather than of the tree: the root keeps its answer and its flag, the
        // rung below it does not, and a deeper rung inherits the nearer decision.
        let mut f = folder("/books");
        f.set_tracking("", true);
        assert!(f.tracked() && f.tracks_rung("Fiction") && f.tracks_rung("Fiction/SciFi"));
        f.set_tracking("Fiction", false);
        assert!(f.tracked(), "the tree is still watched");
        assert!(f.opts.watch, "and the flag still says so");
        assert!(!f.tracks_rung("Fiction"), "the rung turned off is off");
        assert!(!f.tracks_rung("Fiction/SciFi"), "and so is everything below it");
        assert!(f.tracks_rung("Poetry"), "a sibling is untouched");
        // Clearing the rung's decision hands it back to the root.
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
        // A hand-edited blob, or a future format removed from the registry,
        // must not silently turn a watched folder into a dead one.
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
        // `rel` is normalised to `/` by the walk, so a `\` in a key means the
        // blob was written by something that did not normalise it — and that
        // key would never match a found file again.
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
        // A dead pointer is a rung the walk REUSES instead of minting: the chain
        // hands back an id nothing wears, and every placement that rides it
        // lands on no shelf at all — a run that counts a book it filed nowhere.
        // One shelf, held rather than handed over: the prune is asked of the
        // same list twice, and an array literal would move it the first time.
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
        // A tree whose shelves have all gone has no rung left to reuse, so the
        // next walk mints every one of them on the seats the disk names.
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
        // Through the writer rather than the field: `set_tracking` is what keeps
        // the tree and the flag agreed, and a test that poked the flag directly
        // would pass while proving nothing about the folder every surface reads.
        find_mut(&mut folders, "f2").unwrap().set_tracking("", true);
        assert!(find(&folders, "f2").is_some_and(|f| f.tracked() && f.opts.watch));
    }

    /// A folder with the two mode switches set explicitly, where the default
    /// fixture's `false` watch would answer every case the same way.
    ///
    /// The watch arrives as a TREE rather than as the flag alone, because the
    /// flag is now the root rung's mirror: a fixture that set only the flag
    /// would be a folder this build never writes, and a reader of the tree
    /// would answer `false` for a folder the test meant to be watched.
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
