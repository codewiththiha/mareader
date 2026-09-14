//! The folder run: scan one watched folder, run the ledger over what the
//! walk found, copy whatever the options say to copy, and write the result in
//! one go. The stages are the functions below; [`run_folder`] is the order
//! they run in.

use std::collections::{BTreeMap, HashMap, HashSet};

use leptos::prelude::*;

use library_core::book::{add_book, book_rows, book_rows_mut, Book, Fingerprint, Origin, Row};
use library_core::conflict::Arrival;
use library_core::folder::{FolderMode, FolderOpts, WatchedFolder};
use library_core::tracking::TrackingTree;
use library_core::id;
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};
use reader_core::format::Format;

use super::copy::{copy_batch, measure_stores};
use super::gate::{run_fold, RootPlan};
use super::restore::take_represented;
use super::tasks::{fail, finish_task, push_task, update_task, FailMode};
use super::{rel_of, shelf_name, Asked};
use crate::services::library::conflict::{self, ConflictAsk};
use crate::services::library::covers;
use crate::services::library::reveal;
use crate::services::library::folder_label;
use crate::services::library as wire;
use crate::state::library::{ImportTask, NoteKind};
use crate::state::AppState;
use crate::time::now_ms;

/// The shelf a found file belongs on: every rung between the folder's root shelf
/// and the file's own subfolder, minted or reused, with each rung this call minted
/// collected into `new_shelves` for the caller to put a shelf row under.
///
/// One spelling for the two loops a folder run mints through — the books it adds
/// and the books the library already held, which a planned tree owes a membership
/// of — because the two have to agree about what a rung is CALLED and about who
/// OWNS it, and a second copy was a second answer to both. An *as new* run that
/// named its root rung one way for new files and another for known ones would
/// have minted two trees side by side instead of one.
///
/// The whole chain rather than the leaf, which is `shelf_chain_for`'s own rule:
/// importing "1" whose inside is "2", "3" and four books has to produce "1" at the
/// root with "2", "3" and the four books inside it — not three siblings at the
/// root and the books twice.
fn chain_for(
    folder: &mut WatchedFolder,
    key: &str,
    now: u64,
    root: &str,
    planned_name: &Option<String>,
    merged: bool,
    new_shelves: &mut Vec<Shelf>,
) -> String {
    let folder_id = folder.id.clone();
    folder.shelf_chain_for(
        key,
        |_| id::next_shelf_id(now),
        |rung| match (rung.is_empty(), planned_name) {
            // The folder sheet's *as new* answer: the root rung wears the counter
            // name it promised, and every rung below it keeps the disk's own.
            (true, Some(name)) => name.clone(),
            _ => shelf_name(rung, root),
        },
        |rung, id, name, parent| {
            new_shelves.push(Shelf {
                id: id.to_string(),
                name,
                kind: ShelfKind::Folder {
                    folder_id: folder_id.clone(),
                    rel: rel_of(rung),
                },
                books: Vec::new(),
                parent,
                // Minted by the scan, so the scan owns its rung — until a hand
                // moves it, which is `reparent`'s mark. A merged run marks every
                // rung it mints: their tree hangs off a shelf the disk does not
                // own, so there is no disk shape for a re-hang to put them back on.
                manual_parent: merged,
            });
        },
    )
}

// ---------------------------------------------------------------------------
// The dock's cards. Written from here rather than from the dock: the dock is a
// view, and a view that owned the lifecycle of the thing it renders would have
// to outlive the import it is reporting on.
// ---------------------------------------------------------------------------

/// Measure the rows the walk found at addresses the library already holds,
/// taking those files out of the add list. The heal half of the migrated-row
/// rule: a file at an address the library reads IS that book, whatever the two
/// fingerprints say, and the walk has just made the measurement the startup
/// pass could not. Answers the addresses it healed, which is the count the card
/// reports and the set the merge stage passes by: a healed row's membership is
/// the reader's business, exactly as the landing's own heal leaves it.
pub(super) fn heal_by_address(
    books: &mut [Row],
    adds: &mut Vec<FoundFile>,
    skip: &HashSet<String>,
) -> HashSet<String> {
    let mut healed = HashSet::new();
    adds.retain(|file| {
        // A copies run's file is an add BECAUSE the library holds the
        // address: healing it into the row that is there would answer the
        // second instance the reader asked for with the first one.
        if skip.contains(&file.path) {
            return true;
        }
        match book_rows_mut(books).find(|b| b.path() == file.path) {
            Some(book) => {
                book.heal(file.fp);
                healed.insert(file.path.clone());
                false
            }
            None => true,
        }
    });
    healed
}

/// The library as one folder run sees it: the snapshot the ledger diffs against,
/// the walk's own answer, and the set of addresses the run owes a copy of its
/// own.
///
/// A value so the stages below read one consistent picture rather than each
/// taking its own borrow of four locals. Nothing in it is written back — what
/// lands is applied to the LIVE signals at the end, so a book the reader opened
/// while the walk was running is not overwritten by one.
///
/// `pub(super)` for the same reason [`Landing`] is: the stages are the run's
/// units, and a stage a host test cannot build a picture for is a stage whose
/// rule is only ever asserted by reading it.
pub(super) struct Snapshot<'a> {
    pub(super) books: &'a [Row],
    pub(super) registry: &'a ledger::Registry,
    pub(super) found: &'a [FoundFile],
    pub(super) copy_paths: &'a HashSet<String>,
}

/// The folder's ledger row for this run.
///
/// Importing a folder the library already watches continues that row's `placed`
/// and `ignored` sets, which is the whole point of them: re-importing is how a
/// reader would otherwise get back every book they deleted last week. A folder
/// the library has never seen is minted here, and a minted row carries no
/// history for any rule to misread.
///
/// The options are the sheet's. The watch is too, and where its answer LANDS
/// is the run's: a fresh folder's root for an ordinary import, the picked rung
/// for a continuation of a standing tree (written before the run, by
/// `import_folder`), and nowhere at all for a copies run, which the sheet never
/// asks. Turning a standing tree's watch by hand is the shelf's own menu
/// (`crate::services::library::set_shelf_watch`).
pub(super) fn resolve_folder(
    folders: &[WatchedFolder],
    shelves: &[Shelf],
    root: &str,
    opts: FolderOpts,
    plan: &RootPlan,
) -> WatchedFolder {
    let standing = folders.iter().find(|f| f.root == root);
    let mut folder = standing.cloned().unwrap_or_else(|| WatchedFolder {
        id: id::next_folder_id(now_ms()),
        root: root.to_string(),
        opts: opts.clone(),
        placed: HashSet::new(),
        ignored: Vec::new(),
        shelf_map: BTreeMap::new(),
        last_seen: Vec::new(),
        scanned_ms: 0,
        tracking: TrackingTree::default(),
    });
    // The sheet's answers are this import's truth, and the next scan's — with
    // one run that does not own the root's answer. A CONTINUATION of a standing
    // tree is a re-pick of ground the tree already covers, and the sheet's
    // switch on covered ground answered for the RUNG the pick names — an
    // answer `import_folder` wrote on that rung (`set_rung_tracking`) before
    // this run started. The tree's root is not this run's to rewrite: a rung
    // re-picked with the switch on is a question about that subfolder, and
    // writing the sheet's value over the root would turn the WHOLE tree on —
    // or off — from a sheet that promised it answers for the subfolder alone.
    //
    // The root force this replaced — a seated ground forced watched whatever
    // the sheet said — was the lock era's rule, from when the sheet's switch
    // was DISABLED on covered ground and an arriving `false` was a stale
    // default to defend against. The sheet now opens SEEDED with the tree's own
    // answer for the ground, so what arrives is the reader's rather than a
    // leftover: a re-import with the switch off is the reader turning the watch
    // off, and forcing it back on was a switch that did not work.
    //
    // A run that COPIES writes no root decision either, because the watch is
    // not offered beside a copy — and there is nothing in a copying folder's
    // tree for a later scan to honour (see `WatchedFolder::owes_walk`). The
    // mode is read off the folder rather than off the parameter, which was
    // moved into it a line above.
    folder.opts = opts;
    // The sheet's switch is a question about the ROOT RUNG, and the tree is what
    // answers it from here on — so the sheet's answer is written into the tree
    // rather than left on the flag alone. The two stay agreed because
    // `set_tracking` mirrors the root's answer back onto the flag the sheet and
    // the older surfaces read. A continuation re-mirrors the flag from the tree
    // it did not rewrite, so a folder's persisted flag cannot drift from its
    // tree whichever kind of run touched it last.
    if folder.mode().reads_in_place() {
        if plan.continuation.is_none() {
            folder.set_tracking("", folder.opts.watch);
        } else {
            folder.opts.watch = folder.tracking.tracked();
        }
    }
    // A pointer at a shelf that is gone is a rung the walk reuses instead of
    // minting, and every placement that rides it lands on no shelf at all. Cut
    // before the map is written to below, so a merge's root and an *as new*
    // run's clearing are answers about the map that is left.
    folder.prune_shelf_map(shelves);
    // A merge files into the shelf the level already held: the folder's root
    // rung is that shelf, and the map is the one place the walk, the chain
    // minting and every later rescan read the answer from — which is what makes
    // the merge a promise the next scan keeps.
    if let Some(into) = &plan.into {
        folder.shelf_map.insert(String::new(), into.clone());
    }
    // An *as new* answer owes a tree of its OWN: every rung is minted fresh
    // under the counter-named root rather than reusing the rungs that hang off
    // the shelf the folder used to file onto — and from this run on, the new
    // tree is the folder's tree, which is what the map records.
    if plan.rename.is_some() {
        folder.shelf_map.clear();
    }
    folder
}

/// Hang this folder's shelves back on the rungs their `rel` names.
///
/// The tree on disk is the tree on the shelf, for the shelves this folder owns —
/// the rule and its edge cases (a hand-moved shelf keeps its place, a subtree
/// re-hangs together, a virtual shelf is never touched) are
/// `library_core::shelf::rehang_moves`', pure and host-tested; this is the one
/// pass that asks it and applies the answer.
///
/// Before the diff and before the "nothing changed" return on purpose: a library
/// arranged by an older build is repaired by the first rescan that looks at the
/// folder, not only by an import that happens to add something.
fn rehang(state: AppState, folder_id: &str) {
    let moves = state
        .library
        .shelves
        .with_untracked(|shelves| shelves_ops::rehang_moves(shelves, folder_id));
    if moves.is_empty() {
        return;
    }
    state.library.shelves.update(|shelves| {
        for (id, want) in &moves {
            if let Some(shelf) = shelves_ops::find_mut(shelves, id) {
                shelf.parent = want.clone();
            }
        }
    });
    crate::storage::persist_library(state.library);
}

/// The placements a PLANNED tree owes the files the library already holds, and
/// the per-file questions a standing rung's name asks on the way.
///
/// A planned tree — the folder sheet's *as new* or *merge* answer — owes a
/// placement for EVERY file the walk found that the library already holds: a
/// membership of the row it holds it in, never a second row, because one content
/// is one identity and one identity is one row. Those files are exactly the ones
/// the ledger's table answers with a Skip, so a tree promised as "its own shelf,
/// its own tree" would otherwise hold only the new files — and a re-import of one
/// folder, whose every file is known, would hold nothing at all.
///
/// A merge asks before it places: a file whose name a STANDING rung holds — the
/// root shelf the answer named, or any subfolder shelf a previous run mapped — is
/// the compact sheet's per-file question, even when the row wearing the name is
/// the row the file resolves to, because a reader who re-imports a folder to
/// reconcile it is owed the three answers per file wherever the names meet, not a
/// card that says nothing was new. Files no standing name collides with join
/// silently: new books in a merged folder are the default, not a case.
fn planned_placements(
    state: AppState,
    snap: &Snapshot<'_>,
    folder: &WatchedFolder,
    plan: &RootPlan,
    adds: &mut Vec<FoundFile>,
) -> (Vec<(String, FoundFile)>, Vec<ConflictAsk>) {
    if plan.rename.is_none() && plan.into.is_none() {
        return (Vec::new(), Vec::new());
    }
    // Known content never reaches a planned run's add list: its placement is the
    // membership below, and an add would either resolve to the same row twice or
    // — in a copying folder — make a store copy nothing reads. The copies run's
    // own files are the exception the rule exists for: a second instance the
    // reader just asked for, of a file a tree keeps reading in place.
    adds.retain(|f| {
        !snap.registry.contains_key(&f.fp) || snap.copy_paths.contains(&f.path)
    });
    let shelves_now = state.library.shelves.get_untracked();
    let mut replacements = Vec::new();
    let mut asks = Vec::new();
    for file in snap.found {
        // The row the library holds this file in: by content identity first (the
        // ledger's own answer), and by address second for the migrated row whose
        // placeholder identity no measurement matched.
        let known = snap
            .registry
            .get(&file.fp)
            .map(|k| k.id.clone())
            .or_else(|| {
                book_rows(snap.books)
                    .find(|b| b.path() == file.path)
                    .map(|b| b.id.clone())
            });
        let Some(row_id) = known else {
            continue;
        };
        // A file the copies run owes lands as a book of its own below, not as
        // a membership of the row it duplicates.
        if snap.copy_paths.contains(&file.path) {
            continue;
        }
        let key = folder.shelf_key(file);
        let target = plan.into.as_deref().and_then(|into| {
            if key.is_empty() {
                Some(into.to_string())
            } else {
                folder.shelf_map.get(&key).cloned()
            }
        });
        if let Some(target) = target {
            let arrival = Arrival::import(file.clone(), target, None);
            if let Some(existing_id) =
                library_core::conflict::collide(snap.books, &shelves_now, &arrival)
            {
                let existing_name =
                    conflict::existing_name_of(snap.books, &existing_id, &arrival);
                asks.push(ConflictAsk::folder_merge(
                    arrival,
                    existing_id,
                    existing_name,
                    folder.mode(),
                    folder.id.clone(),
                ));
                continue;
            }
        }
        replacements.push((row_id, file.clone()));
    }
    (replacements, asks)
}

/// The compact sheet's questions over the walk's genuinely NEW files.
///
/// The same question [`planned_placements`] asks of the files the library already
/// held, asked of the ones it did not: a merge's new arrivals land on a rung that
/// may already hold their NAME, and a collision there is the compact sheet's too
/// (one book / replace / as new, one at a time or one answer for all). Everything
/// else goes in without being asked — new books are the default, not a case. A new
/// file inside a SUBFOLDER a previous run mapped is asked against that subfolder's
/// shelf; a file in a rung being minted fresh has nothing standing to collide with.
///
/// Only a merge asks. An *as new* tree mints every rung it files into, so there is
/// nothing standing for a name to collide with.
fn screen_merge_adds(
    state: AppState,
    snap: &Snapshot<'_>,
    folder: &WatchedFolder,
    plan: &RootPlan,
    adds: &mut Vec<FoundFile>,
) -> Vec<ConflictAsk> {
    let Some(into) = plan.into.clone() else {
        return Vec::new();
    };
    let shelves_now = state.library.shelves.get_untracked();
    let mut asks = Vec::new();
    adds.retain(|file| {
        let key = folder.shelf_key(file);
        let target = if key.is_empty() {
            Some(into.clone())
        } else {
            folder.shelf_map.get(&key).cloned()
        };
        let Some(target) = target else {
            return true;
        };
        let arrival = Arrival::import(file.clone(), target, None);
        match library_core::conflict::collide(snap.books, &shelves_now, &arrival) {
            Some(existing_id) => {
                let existing_name = conflict::existing_name_of(snap.books, &existing_id, &arrival);
                asks.push(ConflictAsk::folder_merge(
                    arrival,
                    existing_id,
                    existing_name,
                    folder.mode(),
                    folder.id.clone(),
                ));
                false
            }
            None => true,
        }
    });
    asks
}

/// The walked files this folder's tree owes a membership of and does not have
/// one for: a file whose row the library still holds, but on no shelf this
/// folder owns.
///
/// The merge half of a re-pick, and the half the ledger's table cannot answer.
/// Two readers make this shape, and neither is a tombstone:
///
///   * a book the reader filed onto a shelf of their own. The row is where they
///     put it and the file is still on this folder's ground, so both tables
///     answer `Skip` — the content is known and its address has not moved — and
///     the folder's shelf is left holding nothing for a book the folder holds;
///   * a book whose shelf the reader took apart. A shelf is a list of ids and
///     never held a byte, so the row survives it and is in the library on no
///     shelf at all: a book the walk can see and the reader cannot.
///
/// A reader who picks this folder again is asking for its books, so both come
/// back onto the rung their own directory names — and both come back as a
/// MEMBERSHIP rather than a move, which is the folder's own "also show it here":
/// the shelf the reader carried a book to keeps it, because a re-import is an ask
/// about this folder and not an undo of the arrangement beside it. The files this
/// run is minting a row for are not asked (their placement is the landing's own),
/// and neither are the copies a copying run owes, which land as books of their
/// own rather than as memberships of the row they duplicate.
///
/// Empty for a tree whose shelf already holds everything the walk found, which
/// is what leaves the "nothing new" note the answer for a re-pick that really
/// did reconcile a tree with nothing to give back.
pub(super) fn returned_memberships(
    state: AppState,
    snap: &Snapshot<'_>,
    folder_id: &str,
    adds: &[FoundFile],
) -> Vec<(String, FoundFile)> {
    let shelves_now = state.library.shelves.get_untracked();
    let mut out = Vec::new();
    for file in snap.found {
        if adds.iter().any(|add| add.path == file.path) {
            continue;
        }
        // A file the copies run owes lands as a book of its own, not as a
        // membership of the row it duplicates.
        if snap.copy_paths.contains(&file.path) {
            continue;
        }
        // The row the library holds this file in: by content identity first (the
        // ledger's own answer), and by address second for the migrated row whose
        // placeholder identity no measurement matched.
        let Some(row_id) = snap
            .registry
            .get(&file.fp)
            .map(|known| known.id.clone())
            .or_else(|| {
                book_rows(snap.books)
                    .find(|b| b.path() == file.path)
                    .map(|b| b.id.clone())
            })
        else {
            continue;
        };
        // Which shelves a book is on is the shelf module's question, and which of
        // them this folder owns is the shelf KIND's — one spelling of each, the
        // pair the folder's restore menu asks the same way.
        let on_the_tree = shelves_ops::containing(&shelves_now, &row_id)
            .iter()
            .any(|shelf| shelf.kind.folder_id() == Some(folder_id));
        if !on_the_tree {
            out.push((row_id, file.clone()));
        }
    }
    out
}

/// What one walked file needs in order to become a row: the copies that landed,
/// the measurements of the copies run's own, and the shape of the tree to mint
/// into.
///
/// A value rather than eight arguments, because every one of them is a fact about
/// the RUN and not about the file — the file supplies its own address, its
/// measurement and the id minted for it, and everything else is the same for all
/// four hundred of them.
pub(super) struct Landing<'a> {
    pub(super) copies: &'a HashMap<String, String>,
    pub(super) copy_measured: &'a HashMap<String, Fingerprint>,
    pub(super) copy_paths: &'a HashSet<String>,
    pub(super) planned_name: &'a Option<String>,
    pub(super) root: &'a str,
    pub(super) mode: FolderMode,
    pub(super) merged: bool,
    pub(super) now: u64,
}

/// What minting one walked file produced.
pub(super) enum Minted {
    /// A row was placed: its id, and the shelf the chain minted for it.
    Placed { id: String, shelf: String },
    /// The address the file was found at already belonged to a row, so the row
    /// was measured rather than duplicated beside its own twin.
    Healed,
    /// The copy the options owed did not land, so there are no bytes to read and
    /// no row to promise.
    CopyFailed,
}

/// Mint one walked file's row on the LIVE list, or heal the row that appeared at
/// its address while the walk was running.
///
/// The three outcomes are the three things that can be true of a file a walk
/// found, and telling them apart is the run's whole job:
///
///   * a row already reads this address, so the file IS that book and the walk has
///     just made the measurement the startup pass could not. This is the second
///     half of the heal-by-address rule, for the book that appeared while the walk
///     was running. Membership is left alone — the ledger records the placement,
///     and where the reader filed it is the reader's business. The copies run's
///     own files are exempt by name: their twin IS the point of them;
///   * the copy the options owed failed, so there are no bytes to read and a book
///     here would be a card that opens onto an error. The failure is already on
///     the toast the batch raised;
///   * otherwise the file is a new book, and it is minted from the walk's own
///     measurement — a real fingerprint, not a placeholder, so nothing about it is
///     pending.
///
/// A file this folder remembers REMOVING is coming back on an explicit ask, and
/// the reader should not notice it was ever gone: the row returns wearing the name
/// the shelf showed, and the ledger's log is spent by the landing. A copy that
/// fails leaves the log standing, which is the one honest outcome for a file that
/// could not be filed — so the spend happens here, with the landing, and not with
/// the diff.
pub(super) fn mint_walked_row(
    books: &mut Vec<Row>,
    folder: &mut WatchedFolder,
    landing: &Landing<'_>,
    book_id: String,
    file: &FoundFile,
    new_shelves: &mut Vec<Shelf>,
) -> Minted {
    let own_copy = landing.copy_paths.contains(&file.path);
    if !own_copy
        && let Some(existing) = book_rows_mut(books).find(|b| b.path() == file.path)
    {
        existing.heal(file.fp);
        folder.mark_placed(file.fp);
        return Minted::Healed;
    }
    let origin = if landing.mode.reads_in_place() {
        Origin::Linked {
            src: file.path.clone(),
        }
    } else {
        let Some(store) = landing.copies.get(&book_id) else {
            return Minted::CopyFailed;
        };
        Origin::Stored {
            src: Some(file.path.clone()),
            store: store.clone(),
        }
    };
    let stone = ledger::find_tombstone(folder, &file.fp).cloned();
    let store_at = match &origin {
        Origin::Stored { store, .. } => Some(store.clone()),
        Origin::Linked { .. } => None,
    };
    let mut book = Book::new(
        book_id,
        file.fp,
        file.format().unwrap_or(Format::Pdf),
        origin,
        landing.now,
    );
    if let Some(title) = stone.as_ref().and_then(|s| s.title.clone()) {
        book.title = Some(title);
    }
    // The copy list's file is a book of its own beside the linked book the
    // tree keeps: independent, so its marks and its place in it are its own,
    // and known by its copy's measurement — or by the pending flag the startup
    // sweep finishes, when the copy could not be weighed. `add_book`'s
    // one-row-per-fingerprint rule is the right rule for a walk and the wrong
    // one for a second instance the reader just asked for by name, so the copy
    // is pushed past it.
    //
    // A read-at-place book coming back beside the library's copy of ITS OWN
    // file is pushed past the same rule, and for the same shape of reason: one
    // content, two rows, each with one address. The copy wears the fingerprint
    // on a host that stamps a copy like its source, so `add_book` would answer
    // with the copy's id — and the walk would then file the COPY on this rung
    // and report a book come home that never left the store. The link the
    // folder reads and the copy the reader moved out are the shape a departure
    // leaves behind on purpose, so the link is minted as its own row. A
    // COPYING folder keeps the rule whole for the files it already copied: a
    // second copy of a content the library holds as its OWN is the orphan in
    // the store the rule exists to prevent — the copies this run owes are of
    // files the library READS, and they come through the copy list above.
    let beside_its_own_copy = landing.mode.reads_in_place()
        && book_rows(books).any(|b| {
            !b.independent && b.fp == file.fp && b.origin.is_store_copy_of(&file.path)
        });
    let placed_id = if own_copy {
        book.independent = true;
        book.adopt_measurement(
            store_at
                .as_ref()
                .and_then(|store| landing.copy_measured.get(store))
                .copied(),
        );
        let id = book.id.clone();
        books.push(Row::Book(book));
        id
    } else if beside_its_own_copy {
        let id = book.id.clone();
        books.push(Row::Book(book));
        id
    } else {
        add_book(books, book)
    };
    // The whole chain, not the leaf: importing "1" whose inside is "2", "3" and
    // four books has to produce "1" at the root with "2", "3" and the four books
    // inside it — not three siblings at the root and the books twice.
    let key = folder.shelf_key(file);
    let shelf_id = chain_for(
        folder,
        &key,
        landing.now,
        landing.root,
        landing.planned_name,
        landing.merged,
        new_shelves,
    );
    folder.mark_placed(file.fp);
    // The book landed, so a removal that was holding it out is spent.
    ledger::restore_deleted(folder, &file.fp);
    Minted::Placed {
        id: placed_id,
        shelf: shelf_id,
    }
}

/// What the walk owes, decided: the adds (already deduped, healed and
/// screened), the relinks, the planned tree's memberships, the questions a
/// merge asks, and the files a moved-out log represents. One value, so the
/// stages after the diff read one answer rather than eight locals, and the
/// "nothing to do" test is a question of this struct rather than of the
/// run's guts.
struct WalkPlan {
    adds: Vec<FoundFile>,
    relinks: Vec<(String, String)>,
    relinked: usize,
    healed: usize,
    replacements: Vec<(String, FoundFile)>,
    asks: Vec<ConflictAsk>,
    copy_paths: HashSet<String>,
    represented: Vec<String>,
}

/// The diff stage: everything between the walk's raw findings and the copy
/// batch — the registry the ledger reads, the ledger's own two tables, the
/// heal of the rows the walk re-measured, the planned tree's memberships, the
/// memberships a reconciliation owes the books its tree stopped holding, and
/// the questions a merge asks. Decides against the SNAPSHOT; the only thing
/// it writes is the folder's own ledger row, which the run holds.
#[allow(clippy::too_many_arguments)]
fn plan_the_walk(
    state: AppState,
    folder: &mut WatchedFolder,
    books: &mut Vec<Row>,
    found: &mut Vec<FoundFile>,
    asked: Asked,
    plan: &RootPlan,
    quiet: bool,
) -> WalkPlan {
    let registry = ledger::registry_of(books);

    // The copy list, read off the same snapshot the diff reads and before the
    // run writes anything: the addresses whose file the library already reads
    // IN PLACE, and where this run lands a copy of its own beside the linked
    // row. An explicit COPIES run owes every one of them a book — a copies
    // import is the library's own second instance, unrelated to the tree that
    // reads the ground, and a walk that answered Skip over all of them is the
    // silent "Imported 0 books" this list exists to prevent. A RESCAN never
    // owes one: staying quiet about ground another folder placed is the
    // rescan's whole job, and the copies a previous import made are known by
    // their own bytes rather than by these addresses.
    let copy_paths: HashSet<String> = if folder.mode().copies_files() && !quiet {
        ledger::copy_over_paths(found, &registry, books)
    } else {
        HashSet::new()
    };

    // A fingerprint can rejoin the library by any route — a hand-open, a second
    // folder's import, a restore — and a tombstone left behind for a book that
    // exists is a restore row offering something the reader already has.
    ledger::prune_tombstones(folder, &registry);
    // Written on every scan, including one that changes nothing: the restore
    // menu's "did this book move out of my folder" answer is only as fresh as the
    // last walk, and a walk that found nothing to do still saw every file.
    folder.record_seen(found);

    // Explicit runs only — a rescan never reveals, so it never asks.
    let represented: Vec<String> = if quiet {
        Vec::new()
    } else {
        take_represented(state, Some(&folder.id), found)
    };

    let mut adds: Vec<FoundFile> = Vec::new();
    let mut relinks: Vec<(String, String)> = Vec::new();
    // Two tables, one question each: what should come back on its own, and what
    // the reader is asking for right now. See `ledger` for which rows differ.
    let actions = match asked {
        Asked::OnFocus => ledger::diff_folder(folder, &registry, found),
        Asked::Explicitly => ledger::diff_import(folder, &registry, found),
    };
    for action in actions {
        match action {
            ScanAction::Add(file) => adds.push(file),
            ScanAction::Relink { book_id, to } => relinks.push((book_id, to)),
            ScanAction::Skip => {}
        }
    }
    // A relink that would point a book at an address another row already reads
    // is a relink of the WRONG row — the ledger owns that rule and its test.
    ledger::keep_healable_relinks(&mut relinks, books);
    let relinked = relinks.len();

    // The ledger answered Skip for the copy run's own files — their content is
    // known — but the run owes each of them a book of its own: back onto the
    // add list they go, and the planned-tree pass below keeps them out of the
    // memberships it owes the OTHER known files.
    if !copy_paths.is_empty() {
        for file in found
            .iter()
            .filter(|f| copy_paths.contains(&f.path))
        {
            if !adds.iter().any(|a| a.path == file.path) {
                adds.push(file.clone());
            }
        }
    }

    // The snapshot every stage below reads, so they all decide against one
    // picture of the library rather than each taking its own borrow of four
    // locals. Dropped by the heal underneath it, which writes to `books`.
    let (mut replacements, mut asks) = {
        let snap = Snapshot {
            books,
            registry: &registry,
            found,
            copy_paths: &copy_paths,
        };
        planned_placements(state, &snap, folder, plan, &mut adds)
    };

    // A file at an address the library already holds IS that book, whatever the
    // two fingerprints say. The case this catches is a row migrated from the
    // previous schema: it carries a placeholder identity because nothing ever
    // measured it, so the ledger above saw "unknown content" — and adding it
    // would put a second copy of the same file on the shelf next to its own
    // twin. Healing the row is the honest answer, and the walk has just made the
    // measurement the startup pass could not.
    let healed_paths = heal_by_address(books, &mut adds, &copy_paths);
    let healed = healed_paths.len();

    // One book per fingerprint INSIDE a single scan, always: a tree holding two
    // byte-identical files is one book, and copying both would leave an orphan
    // in the store that nothing can ever remove. Across scans it is the
    // RESCAN's rule and not an explicit import's — a reader who asks for this
    // folder is asking for the files in it, and a byte-identical copy of a book
    // another folder placed is still a file this folder holds, so it is still a
    // book on this folder's shelf.
    let mut seen: HashSet<Fingerprint> = match asked {
        Asked::OnFocus => registry.keys().copied().collect(),
        Asked::Explicitly => HashSet::new(),
    };
    adds.retain(|f| seen.insert(f.fp));

    // The same question for the files the ledger has NOT seen, which a merge
    // asks and an *as new* tree has nothing standing to ask it against.
    {
        let snap = Snapshot {
            books,
            registry: &registry,
            found,
            copy_paths: &copy_paths,
        };
        asks.extend(screen_merge_adds(state, &snap, folder, plan, &mut adds));
    }

    // The reconciliation's merge, which is the half of a re-pick that is not an
    // add: the walked files this folder's tree owes a membership of and does not
    // have one for. Asked last, once `adds` is the list that is actually going
    // to be minted, so a file this run is making a row for is not also asked for
    // a membership of the row it is about to replace.
    if !quiet
        && folder.mode().reads_in_place()
        && replacements.is_empty()
        && plan.rename.is_none()
        && plan.into.is_none()
    {
        let folder_id = folder.id.clone();
        let mut returned = {
            let snap = Snapshot {
                books,
                registry: &registry,
                found,
                copy_paths: &copy_paths,
            };
            returned_memberships(state, &snap, &folder_id, &adds)
        };
        // A row the walk just healed keeps the membership it had, which is the
        // landing's own rule for a heal, and counting it here as well would be
        // counting one file twice on the card that reports the run.
        returned.retain(|(_, file)| !healed_paths.contains(&file.path));
        // The folder's shelf holds these files from this run on, which is what
        // `placed` says: a book removed after it is a removal THIS folder takes
        // a tombstone for, and a fingerprint the ledger skips with no book
        // behind it is the one state a folder cannot recover from on its own.
        for (_, file) in &returned {
            folder.mark_placed(file.fp);
        }
        replacements = returned;
    }

    WalkPlan {
        adds,
        relinks,
        relinked,
        healed,
        replacements,
        asks,
        copy_paths,
        represented,
    }
}

/// What the landing stage counted.
struct LandTally {
    placed: u32,
    relinked: usize,
    healed: usize,
}

/// The landing stage: the diff's answer, written to the LIVE lists rather
/// than to the snapshot — a walk of a big folder takes seconds, and a reader
/// who opens a book during one must not have that read overwritten by the
/// write at the end. `update` re-reads inside the write, so an import lands
/// on top of whatever happened while it was walking. The folder's ledger row
/// and the blob write stay with the caller: a stage that wrote half a library
/// would be a stage the next one could not trust.
#[allow(clippy::too_many_arguments)]
fn land_the_walk(
    state: AppState,
    folder: &mut WatchedFolder,
    walk: &mut WalkPlan,
    pending: Vec<(String, &FoundFile)>,
    copies: &HashMap<String, String>,
    copy_measured: &HashMap<String, Fingerprint>,
    root: &str,
    planned_name: &Option<String>,
    merged: bool,
    now: u64,
) -> LandTally {
    // Applied to the LIVE lists rather than to the copies taken before the scan.
    // A walk of a big folder takes seconds, and a reader who opens a book during
    // one would otherwise have that read overwritten by the write at the end — or,
    // if the book was new to the library, dropped from it entirely. `update`
    // re-reads inside the write, so an import lands on top of whatever happened
    // while it was walking.
    let mut placed = 0u32;
    let mut relink_count = 0usize;
    let mut healed_here = 0usize;
    let mut new_shelves: Vec<Shelf> = Vec::new();
    let mut placements: Vec<(String, String)> = Vec::new();
    let mode = folder.mode();
    // Taken BEFORE the landing borrows the walk's copy list: a mutable take
    // under an outstanding shared borrow is a borrow the compiler refuses,
    // and the relink list is the one field the landing consumes rather than
    // reads.
    let relinks = std::mem::take(&mut walk.relinks);
    let landing = Landing {
        copies,
        copy_measured,
        copy_paths: &walk.copy_paths,
        planned_name,
        root,
        mode,
        merged,
        now,
    };

    state.library.books.update(|books| {
        for (book_id, to) in relinks {
            if ledger::relink(books, &book_id, &to) {
                relink_count += 1;
            }
        }
        for (book_id, file) in pending {
            match mint_walked_row(books, folder, &landing, book_id, file, &mut new_shelves) {
                Minted::Placed { id, shelf } => {
                    placements.push((id, shelf));
                    placed += 1;
                }
                Minted::Healed => healed_here += 1,
                Minted::CopyFailed => {}
            }
        }
        // The planned tree's other half: the folder's books the library
        // already held, as memberships of the rows it holds them in. The
        // chain mints whatever rungs are not in the map yet — the whole tree
        // of an *as new* run, nothing at all of a merge into shelves that
        // stand — and the member guard below keeps a book that is already
        // where it is being put from moving to the end of it.
        for (row_id, file) in &walk.replacements {
            let key = folder.shelf_key(file);
            let shelf_id = chain_for(
                folder,
                &key,
                landing.now,
                landing.root,
                landing.planned_name,
                landing.merged,
                &mut new_shelves,
            );
            placements.push((row_id.clone(), shelf_id));
        }
    });

    state.library.shelves.update(|shelves| {
        for shelf in new_shelves {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        for (book_id, shelf_id) in &placements {
            let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) else {
                continue;
            };
            // Two byte-identical files in one tree are one book, so the second
            // resolves to an id that is already a member: appending it again would
            // reshuffle the shelf the reader can see.
            if !shelf.books.iter().any(|m| m == book_id) {
                shelves_ops::place(&mut shelf.books, book_id, None);
            }
        }
    });


    LandTally {
        placed,
        relinked: relink_count,
        healed: healed_here,
    }
}

/// Scan one folder, run the ledger over what the walk found, copy whatever the
/// options say to copy, and write the result in one go.
///
/// The order the stages run in, rather than any of them: [`resolve_folder`]
/// says which ledger row the walk continues, [`rehang`] puts the folder's
/// shelves back on the rungs their directories name, [`plan_the_walk`]
/// decides what the walk owes ([`ledger::diff_folder`] for a rescan and
/// [`ledger::diff_import`] for a run the reader asked for, which differ about
/// tombstones and nothing else), the copy batch makes whatever bytes the
/// options say to make, [`land_the_walk`] lands all of it on the live signals
/// at once, and the tail reports — the fold, the light, the questions and
/// the card.
pub(super) async fn run_folder(
    state: AppState,
    task: String,
    root: String,
    opts: FolderOpts,
    asked: Asked,
    plan: RootPlan,
) {
    // A rescan is the quiet half of this function: it owes the reader no card
    // and no write for a folder nothing changed in. An import owes an answer
    // either way.
    let quiet = asked == Asked::OnFocus;
    let fail_mode = if quiet {
        FailMode::ConsoleOnly
    } else {
        FailMode::Toast
    };
    let mut found = match wire::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, fail_mode),
    };

    // A snapshot, and only a snapshot: the ledger needs a consistent library to
    // diff against, but nothing below writes these copies back. What lands is
    // applied to the live signals at the end, so a book opened while the walk was
    // running is not overwritten by one.
    let mut books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    let shelves_now = state.library.shelves.get_untracked();

    let mut folder = resolve_folder(&folders, &shelves_now, &root, opts, &plan);
    rehang(state, &folder.id);

    // The diff stage: what the walk owes, decided against the snapshot.
    let mut walk = plan_the_walk(state, &mut folder, &mut books, &mut found, asked, &plan, quiet);

    if walk.adds.is_empty()
        && walk.relinked == 0
        && walk.healed == 0
        && walk.asks.is_empty()
        && walk.replacements.is_empty()
        && walk.represented.is_empty()
    {
        // Nothing to do. A quiet run leaves no trace beyond the folder's own
        // "last scanned" stamp; an explicit import still owes the reader an
        // answer, which is a card saying nothing was new — and a re-pick of
        // ground the library already reads in place, the tree's root or any
        // rung of it, owes the note as well: a sentence earned by a walk that
        // found every book already standing.
        //
        // One shape of "nothing new" is not the note's, and it is an ANSWER
        // rather than a question: a member of this tree is standing OUTSIDE
        // it — its rung was removed and the subfolder imported on its own, or
        // the pick itself is a folder a family could hold. An import is an
        // ask, and a member outside its family is an ask answered: the fold
        // puts it back on the rung its directory names, and the report names
        // the shelf that went home.
        folder.scanned_ms = now_ms();
        let root_rung = folder.shelf_map.get("").cloned();
        let folder_id = folder.id.clone();
        write_folder(state, folder);
        if !quiet {
            let folded = state
                .library
                .folder(&folder_id)
                .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found));
            match folded {
                Some((shelf_id, name)) => {
                    conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
                }
                None => {
                    if let Some((shelf_id, name)) = plan.continuation.clone() {
                        conflict::raise_note(state, shelf_id, name, NoteKind::NothingNew);
                    }
                }
            }
            update_task(state, &task, |t| t.finish());
        }
        return;
    }

    let now = now_ms();
    // Minted off the crate's own counter rather than the snapshot's length: two
    // watched folders rescan concurrently, and two tasks that both counted the
    // library as it was BEFORE their walks would mint the same id twice in the
    // same millisecond — two books wearing one id, which the next load's
    // sanitize resolves by dropping one of them.
    let adds = std::mem::take(&mut walk.adds);
    let pending: Vec<(String, &FoundFile)> = adds
        .iter()
        .map(|file| (id::next_id(now), file))
        .collect();
    let replaced = walk.replacements.len();
    let expected = (pending.len() + walk.relinked + walk.healed + replaced) as u32;
    if quiet {
        // The first card appears only now, so a focus rescan that found nothing
        // never raises one at all.
        let mut card = ImportTask::new(task.clone(), folder_label(&root));
        card.total = expected;
        push_task(state, card);
    } else {
        update_task(state, &task, move |t| t.total = expected);
    }

    let copies = if folder.mode().reads_in_place() || pending.is_empty() {
        HashMap::new()
    } else {
        match copy_batch(state, &task, &pending).await {
            Ok(copies) => copies,
            Err(message) => return fail(state, &task, message, fail_mode),
        }
    };
    // The copy run's copies are measured in one pass before a row is promised:
    // an independent copy of a file a tree still reads in place must not wear
    // the ORIGINAL's fingerprint — that identity stays the linked book's, and
    // the copy is known by its own bytes, the way every instance the library
    // owns is.
    let copy_measured: HashMap<String, Fingerprint> = if !walk.copy_paths.is_empty() {
        let stores: Vec<String> = pending
            .iter()
            .filter(|(_, file)| walk.copy_paths.contains(&file.path))
            .filter_map(|(book_id, _)| copies.get(book_id).cloned())
            .collect();
        measure_stores(stores).await
    } else {
        HashMap::new()
    };

    // The landing stage, on the live lists.
    let tally = land_the_walk(
        state,
        &mut folder,
        &mut walk,
        pending,
        &copies,
        &copy_measured,
        &root,
        &plan.rename,
        plan.into.is_some(),
        now,
    );

    folder.scanned_ms = now;
    let root_rung = folder.shelf_map.get("").cloned();
    let folder_id = folder.id.clone();
    write_folder(state, folder);
    // The tree the walk just finished is hung on the seats the disk names, in
    // the run that finished it rather than at the start of the next one. The
    // re-hang at the top of this function read the map the walk was about to
    // write, so a rung the map had lost — a root shelf taken apart, its rungs
    // lifted to the level it was on — is a rung this run minted a fresh seat
    // for and then left hanging where the removal put it. Same pass, same rule,
    // and a no-op for a tree that was already on its seats.
    rehang(state, &folder_id);
    crate::storage::persist_library(state.library);
    // A folder import is the case this matters most: it is the one way a shelf
    // arrives with dozens of books at once, and a plate of fallbacks is not a
    // shelf the reader can scan.
    covers::backfill_missing(state);

    // The fold an explicit run owes at its end — the plan's own, which puts
    // the picked folder back into the family its ground names, or the outer
    // tree's, which puts a member found standing outside back inside — runs
    // before any light, so what is revealed is the shelf WHERE IT NOW IS.
    let folded = if quiet {
        None
    } else {
        state
            .library
            .folder(&folder_id)
            .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found))
    };

    // What a FOLDER import reveals is the folder: the shelf the reader's pick
    // named, lit on the level that holds it — the reader stays outside, where
    // the folder is visible, because a folder import is about a folder. For a
    // re-pick of ground the library already reads that is the continuation's
    // shelf — the tree's root for its root, the RUNG for a rung, because the
    // rung is the ground the reader asked about even though the walk that
    // answered ran on the whole tree — and for a first import it is the run's
    // own root shelf. Going inside and lighting a book is what a FILE import
    // does (`run_files`), and a folder's books are not the folder. A fold gets
    // the sentence instead of the bare light: the note says the shelf went
    // home, and its highlight rides the note's close like every note's.
    let represented_count = walk.represented.len() as u32;
    if !quiet {
        match folded {
            Some((shelf_id, name)) => {
                conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
            }
            None => {
                let light = plan
                    .continuation
                    .as_ref()
                    .map(|(shelf_id, _)| shelf_id.clone())
                    .or_else(|| root_rung.clone());
                if let Some(shelf_id) = light {
                    reveal::reveal_shelf(state, &shelf_id);
                }
            }
        }
    }

    // The asks are raised after the clean half landed and the blob was
    // written: the sheet counts against the level as the landing left it, and
    // a card that finishes with questions outstanding says so rather than
    // claiming an import nobody has answered yet.
    let waiting = walk.asks.len() as u32;
    if !walk.asks.is_empty() {
        let asks = std::mem::take(&mut walk.asks);
        conflict::raise(state, asks);
    }

    let healed = walk.healed + tally.healed;
    let total = tally.placed + (tally.relinked + healed + replaced) as u32 + represented_count;
    finish_task(state, &task, total, waiting);
}

/// Put the folder row back. One place, because the ledger is the part of the
/// library that must never be written half-updated: a `placed` set that lost an
/// entry re-adds a book the reader already filed. Written through `update` for the
/// same reason the books are — two imports running at once must not each replace
/// the other's folder row.
fn write_folder(state: AppState, folder: WatchedFolder) {
    state.library.folders.update(|folders| {
        match folders.iter().position(|f| f.id == folder.id) {
            Some(at) => folders[at] = folder,
            None => folders.push(folder),
        }
    });
}
