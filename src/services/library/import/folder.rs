//! The folder run: scan one watched folder, run the ledger over what the walk
//! found, copy whatever the options say to copy, and write the result in one
//! go. The stages are the functions below; [`run_folder`] is their order.
//!
//! Re-filing the books that are ALREADY here, after a shape answer moves them
//! between rungs, is [`super::reshape`]: that pass never reads the disk.

use std::collections::{HashMap, HashSet};

use leptos::prelude::*;

use library_core::book::{Book, Fingerprint, Origin, Row, add_book, book_rows, book_rows_mut};
use library_core::conflict::Arrival;
use library_core::folder::{FolderMode, FolderOpts, WatchedFolder};
use library_core::id;
use library_core::ledger::{self, ScanAction};
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf};

use super::copy::{Landed, copy_batch};
use super::gate::{Continuation, Fold, RootPlan, run_fold, seed_member_rungs};
use super::kept;
use super::reshape::{reshape_row, reshape_the_tree, shape_moved, write_shape};
use super::restore::take_represented;
use super::tasks::{FailMode, fail, finish_task, push_task, run_total, update_task};
use super::{Asked, rel_of, rung_label};
use crate::services::library as ipc;
use crate::services::library::conflict::{self, ConflictAsk};
use crate::services::library::covers;
use crate::services::library::folder_label;
use crate::services::library::reveal;
use crate::state::AppState;
use crate::state::library::{ImportTask, NoteKind};
use crate::time::now_ms;

/// One spelling for the two loops a folder run mints through — the books it
/// adds and the books the library already held — so a rung cannot be minted
/// twice under two spellings of its own name.
pub(super) fn chain_for(
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
            // The folder sheet's *as new* answer: the root rung wears the counter name it promised.
            (true, Some(name)) => name.clone(),
            _ => rung_label(rung, root),
        },
        |rung, id, name, parent| {
            let mut minted = Shelf::folder_shelf(id, name, &folder_id, rel_of(rung), parent);
            // Minted by the scan, so the scan owns its rung — until a hand
            // moves it, which is `reparent`'s mark to clear.
            minted.manual_parent = merged;
            new_shelves.push(minted);
        },
    )
}

/// The heal half of the migrated-row rule: a file at an address the library
/// reads is that book, whatever the two fingerprints say.
pub(super) fn heal_by_address(
    books: &mut [Row],
    adds: &mut Vec<FoundFile>,
    skip: &HashSet<String>,
) -> HashSet<String> {
    let mut healed = HashSet::new();
    adds.retain(|file| {
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

/// A value so the stages below read one consistent picture rather than each
/// borrowing four locals. Nothing in it is written back: what lands is
/// applied to the live signals at the end.
pub(super) struct Snapshot<'a> {
    pub(super) books: &'a [Row],
    pub(super) registry: &'a ledger::Registry,
    pub(super) found: &'a [FoundFile],
    pub(super) copy_paths: &'a HashSet<String>,
}

impl Snapshot<'_> {
    /// The row that answers for a found file: by content identity first, which
    /// is the ledger's answer, and by address second for a migrated row whose
    /// placeholder identity no measurement ever matched.
    pub(super) fn known_row(&self, file: &FoundFile) -> Option<String> {
        self.registry
            .get(&file.fp)
            .map(|known| known.id.clone())
            .or_else(|| {
                book_rows(self.books)
                    .find(|b| b.path() == file.path)
                    .map(|b| b.id.clone())
            })
    }
}

/// Importing a folder the library already watches continues that row's
/// `placed` and `ignored` sets — the whole point of them: re-importing is how
/// a reader would otherwise get back every book they deleted last week.
pub(super) fn resolve_folder(
    folders: &[WatchedFolder],
    shelves: &[Shelf],
    root: &str,
    opts: FolderOpts,
    plan: &RootPlan,
) -> WatchedFolder {
    let standing = folders.iter().find(|f| f.root == root);
    let mut folder = standing.cloned().unwrap_or_else(|| {
        WatchedFolder::new(id::next_folder_id(now_ms()), root.to_string(), opts.clone())
    });
    // A continuation of a standing tree is a re-pick of ground the tree
    // covers; the sheet's answers stand for the rung the pick names and are
    // written onto that rung, not onto the tree. The root's own answer is the
    // tree's and is kept here.
    let root_shape = folder.shape_at("");
    folder.opts = opts;
    // The sheet's switch asks about the root rung; the tree answers it from
    // here on. The two stay agreed because `set_tracking` mirrors the root's
    // answer back onto the legacy flag.
    if folder.mode().reads_in_place() {
        if plan.continuation.is_none() {
            folder.set_tracking("", folder.opts.watch);
        } else {
            folder.opts.watch = folder.tracking.tracked();
            folder.opts.groups = root_shape;
        }
    }
    // Cut before the map is written below, so a merge's root and an *as new*
    // run's clearing answer about the map that is left.
    folder.prune_shelf_map(shelves);
    // A merge files into the shelf the level already held: the root rung is
    // that shelf, which makes the merge a promise the next scan keeps.
    if let Some(into) = &plan.into {
        folder.shelf_map.insert(String::new(), into.clone());
    }
    // An *as new* answer owes a tree of its own: every rung is minted fresh
    // under the counter-named root.
    if plan.rename.is_some() {
        folder.shelf_map.clear();
    }
    folder
}

/// The rule and its edge cases are `library_core::shelf::rehang_moves`',
/// pure and host-tested; this is the one pass that asks it and applies the
/// answer. Runs before the walk, so the placements below land on the tree as
/// it now stands.
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

/// A planned tree (*as new* or *merge*) owes a placement for every found file
/// the library already holds: a membership of the row holding it, never a
/// second row — one content is one identity.
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
    adds.retain(|f| !snap.registry.contains_key(&f.fp) || snap.copy_paths.contains(&f.path));
    let shelves_now = state.library.shelves.get_untracked();
    let mut replacements = Vec::new();
    let mut asks = Vec::new();
    for file in snap.found {
        let Some(row_id) = snap.known_row(file) else {
            continue;
        };
        if snap.copy_paths.contains(&file.path) {
            continue;
        }
        if let Some(into) = plan.into.as_deref()
            && let Some(ask) = merge_collision(snap, &shelves_now, folder, into, file)
        {
            asks.push(ask);
            continue;
        }
        replacements.push((row_id, file.clone()));
    }
    (replacements, asks)
}

/// [`planned_placements`]' question asked of the files the library did not
/// hold: a merge's new arrivals land on a rung that may already hold their
/// name, and a collision there is the compact sheet's too.
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
    adds.retain(
        |file| match merge_collision(snap, &shelves_now, folder, &into, file) {
            Some(ask) => {
                asks.push(ask);
                false
            }
            None => true,
        },
    );
    asks
}

/// The merge's question about one file: the rung the merge files it onto, and
/// whether that rung already holds its name. `None` when the merge has no seat
/// for the file's rung, or when the seat is free.
fn merge_collision(
    snap: &Snapshot<'_>,
    shelves_now: &[Shelf],
    folder: &WatchedFolder,
    into: &str,
    file: &FoundFile,
) -> Option<ConflictAsk> {
    let key = folder.shelf_key(file);
    let target = if key.is_empty() {
        into.to_string()
    } else {
        folder.shelf_map.get(&key).cloned()?
    };
    let arrival = Arrival::import(file.clone(), target, None);
    let existing_id = library_core::conflict::collide(snap.books, shelves_now, &arrival)?;
    let existing_name = conflict::existing_name_of(snap.books, &existing_id, &arrival);
    Some(ConflictAsk::folder_merge(
        arrival,
        existing_id,
        existing_name,
        folder.mode(),
        folder.id.clone(),
    ))
}

/// The merge half of a re-pick — the half the ledger's table cannot answer.
/// Two readers make this shape, neither a tombstone: a book filed onto a
/// shelf of the reader's own, and a merge that took the rung a file sat on.
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
        if snap.copy_paths.contains(&file.path) {
            continue;
        }
        let Some(row_id) = snap.known_row(file) else {
            continue;
        };
        // Which shelves a book is on is the shelf module's question; which
        // of them this folder owns is the shelf kind's.
        let on_the_tree = shelves_ops::containing(&shelves_now, &row_id)
            .iter()
            .any(|shelf| shelf.kind.folder_id() == Some(folder_id));
        if !on_the_tree {
            out.push((row_id, file.clone()));
        }
    }
    out
}

/// A value rather than eight arguments: every one is a fact about the run,
/// not about the file.
pub(super) struct Landing<'a> {
    /// The copies that came home, each with its own measurement: the row
    /// adopts it as it is minted, so there is no second pass over the copies.
    pub(super) copies: &'a HashMap<String, Landed>,
    /// Owned rather than borrowed: the landing takes the walk by `&mut` while
    /// the mints read this.
    pub(super) copy_paths: HashSet<String>,
    pub(super) planned_name: &'a Option<String>,
    pub(super) root: &'a str,
    pub(super) mode: FolderMode,
    pub(super) merged: bool,
    pub(super) now: u64,
}

pub(super) enum Minted {
    Placed { id: String, shelf: String },
    Healed,
    CopyFailed,
}

/// The three things that can be true of a found file — a row already reads
/// this address, the file is new, or the owed copy did not land — and telling
/// them apart is the run's whole job.
pub(super) fn mint_walked_row(
    books: &mut Vec<Row>,
    folder: &mut WatchedFolder,
    landing: &Landing<'_>,
    book_id: String,
    file: &FoundFile,
    new_shelves: &mut Vec<Shelf>,
) -> Minted {
    let own_copy = landing.copy_paths.contains(&file.path);
    if !own_copy && let Some(existing) = book_rows_mut(books).find(|b| b.path() == file.path) {
        existing.heal(file.fp);
        folder.mark_placed(file.fp);
        return Minted::Healed;
    }
    let origin = if landing.mode.reads_in_place() {
        Origin::Linked {
            src: file.path.clone(),
        }
    } else {
        let Some((store, _)) = landing.copies.get(&book_id) else {
            return Minted::CopyFailed;
        };
        Origin::Stored {
            src: Some(file.path.clone()),
            store: store.clone(),
        }
    };
    let stone = ledger::find_tombstone(folder, &file.fp).cloned();
    let own_measurement = landing
        .copies
        .get(&book_id)
        .and_then(|(_, measured)| *measured);
    let mut book = Book::new(
        book_id,
        file.fp,
        file.admitted_format(),
        origin,
        landing.now,
    );
    if let Some(title) = stone.as_ref().and_then(|s| s.title.clone()) {
        book.title = Some(title);
    }
    // A copy-list file becomes a book of its own beside the linked book the
    // tree keeps: independent, with its own marks and place. `add_book`'s
    // one-row-per-fingerprint rule is right for a walk and wrong for the
    // second instance the reader just asked for.
    let beside_its_own_copy = landing.mode.reads_in_place()
        && book_rows(books)
            .any(|b| !b.independent && b.fp == file.fp && b.origin.is_store_copy_of(&file.path));
    if own_copy {
        book.independent = true;
        book.adopt_measurement(own_measurement);
    }
    let placed_id = if own_copy || beside_its_own_copy {
        let id = book.id.clone();
        books.push(Row::Book(book));
        id
    } else {
        add_book(books, book)
    };
    // The whole chain, not the leaf: importing "1" containing "2" and four
    // books has to produce "1" at the root with them inside.
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
    ledger::restore_deleted(folder, &file.fp);
    Minted::Placed {
        id: placed_id,
        shelf: shelf_id,
    }
}

/// One value, so the stages after the diff read one answer rather than eight
/// locals.
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
/// batch. Decides against the snapshot; the only thing it writes is the
/// folder's own ledger row.
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

    // Addresses whose file the library already reads in place, where this run
    // lands its own copy beside the linked row: a copies import is the
    // library's second instance, unrelated to the tree reading the ground.
    let copy_paths: HashSet<String> = if folder.mode().copies_files() && !quiet {
        ledger::copy_over_paths(found, &registry, books)
    } else {
        HashSet::new()
    };

    ledger::prune_tombstones(folder, &registry);
    // Written on every scan, including a quiet one: a walk that found
    // nothing to do still saw every file.
    folder.record_seen(found);

    let represented: Vec<String> = if quiet {
        Vec::new()
    } else {
        take_represented(state, Some(&folder.id), found)
    };

    let mut adds: Vec<FoundFile> = Vec::new();
    let mut relinks: Vec<(String, String)> = Vec::new();
    // Two tables, one question each: what should come back on its own, and
    // what the reader is asking for right now.
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
    ledger::keep_healable_relinks(&mut relinks, books);
    let relinked = relinks.len();

    // The ledger answered Skip for the copy run's own files — their content
    // is known — but the run owes each a book of its own.
    if !copy_paths.is_empty() {
        for file in found.iter().filter(|f| copy_paths.contains(&f.path)) {
            if !adds.iter().any(|a| a.path == file.path) {
                adds.push(file.clone());
            }
        }
    }

    let (mut replacements, mut asks) = {
        let snap = Snapshot {
            books,
            registry: &registry,
            found,
            copy_paths: &copy_paths,
        };
        planned_placements(state, &snap, folder, plan, &mut adds)
    };

    // A file at an address the library already holds is that book, whatever
    // the fingerprints say: this catches a migrated row whose placeholder
    // identity nothing ever measured.
    let healed_paths = heal_by_address(books, &mut adds, &copy_paths);
    let healed = healed_paths.len();

    // One book per fingerprint per scan: two byte-identical files are one
    // book, and copying both would leave an orphan in the store nothing can
    // remove.
    let mut seen: HashSet<Fingerprint> = match asked {
        Asked::OnFocus => registry.keys().copied().collect(),
        Asked::Explicitly => HashSet::new(),
    };
    adds.retain(|f| seen.insert(f.fp));

    {
        let snap = Snapshot {
            books,
            registry: &registry,
            found,
            copy_paths: &copy_paths,
        };
        asks.extend(screen_merge_adds(state, &snap, folder, plan, &mut adds));
    }

    // Asked last, once `adds` is the list actually going to be minted.
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
        returned.retain(|(_, file)| !healed_paths.contains(&file.path));
        // A book removed after this is a removal this folder takes a
        // tombstone for: a skipped fingerprint with no book behind it is the
        // one state a folder cannot recover from.
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

/// The shelves a mint reported, added unless already there: a second walk can
/// name a rung the first minted, and the id is what makes a rung the same
/// rung.
pub(super) fn page_shelves(state: AppState, minted: Vec<Shelf>) {
    state
        .library
        .shelves
        .update(|shelves| page_into(shelves, minted));
}

/// [`page_shelves`] for a caller already inside a shelf write: the paging
/// rides the write it has rather than making a second one, which is what
/// `land_the_walk` and the fold's seat owe — one pulse per collection.
pub(super) fn page_into(shelves: &mut Vec<Shelf>, minted: Vec<Shelf>) {
    for shelf in minted {
        if !shelves.iter().any(|s| s.id == shelf.id) {
            shelves.push(shelf);
        }
    }
}

struct LandTally {
    placed: u32,
    relinked: usize,
    healed: usize,
}

/// The diff's answer, written to the live lists rather than the snapshot: a
/// walk of a big folder takes seconds, and a reader who opens a book during
/// one must not have that read overwritten by the write at the end.
/// Everything the mints need about the run arrives in the [`Landing`].
fn land_the_walk(
    state: AppState,
    folder: &mut WatchedFolder,
    walk: &mut WalkPlan,
    pending: Vec<(String, &FoundFile)>,
    landing: &Landing<'_>,
) -> LandTally {
    let mut placed = 0u32;
    let mut relink_count = 0usize;
    let mut healed_here = 0usize;
    let mut new_shelves: Vec<Shelf> = Vec::new();
    let mut placements: Vec<(String, String)> = Vec::new();
    let relinks = std::mem::take(&mut walk.relinks);

    state.library.books.update(|books| {
        for (book_id, to) in relinks {
            if ledger::relink(books, &book_id, &to) {
                relink_count += 1;
            }
        }
        for (book_id, file) in pending {
            match mint_walked_row(books, folder, landing, book_id, file, &mut new_shelves) {
                Minted::Placed { id, shelf } => {
                    // A file that came back is the file that left: what was
                    // kept lands with it.
                    kept::reclaim(books, file, &id);
                    placements.push((id, shelf));
                    placed += 1;
                }
                Minted::Healed => healed_here += 1,
                Minted::CopyFailed => {}
            }
        }
        // The chain mints whatever rungs are not in the map yet, and the
        // member guard below keeps a book already where it is being put from
        // moving to the end of it.
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
        page_into(shelves, new_shelves);
        for (book_id, shelf_id) in &placements {
            let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) else {
                continue;
            };
            shelves_ops::shelf_add(shelf, book_id);
        }
    });

    LandTally {
        placed,
        relinked: relink_count,
        healed: healed_here,
    }
}

/// The order the stages run in: [`resolve_folder`], [`rehang`],
/// [`plan_the_walk`], the copy batch, [`land`], then the questions a merge
/// asks.
pub(super) async fn run_folder(
    state: AppState,
    task: String,
    root: String,
    opts: FolderOpts,
    asked: Asked,
    plan: RootPlan,
) {
    let quiet = asked == Asked::OnFocus;
    let fail_mode = if quiet {
        FailMode::ConsoleOnly
    } else {
        FailMode::Toast
    };
    let mut found = match ipc::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, fail_mode),
    };

    let mut books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    let shelves_now = state.library.shelves.get_untracked();

    // The shape question is about a row and a rung: a fold answers for the
    // rung the pick becomes, a covered re-pick for the rung the sheet lit.
    let answered = plan.answered_rung();
    let shaped = opts.groups;
    let moved_shape = shape_moved(
        &folders,
        &root,
        plan.fold
            .as_ref()
            .map(|Fold { tree_id, .. }| tree_id.as_str()),
        &answered,
        &opts,
    );
    let mut folder = resolve_folder(&folders, &shelves_now, &root, opts, &plan);
    // The reader's answer is written on the row that reads the ground before
    // the walk, so the walk and the fold read the shape this import asks for
    // — and on the run's own row, which is where the run reads the shape it
    // lands books under. A resolved row keeps its root answer while the pick
    // answered for a rung under it.
    if let Some((row, rung)) = &moved_shape {
        write_shape(state, row, rung, shaped);
        folder.set_shape(rung, shaped);
    }
    let own_shape = moved_shape
        .as_ref()
        .filter(|(row, _)| row == &folder.id)
        .map(|(_, rung)| rung.clone());
    let fold_shape = if own_shape.is_some() {
        None
    } else {
        moved_shape.clone()
    };
    rehang(state, &folder.id);
    // With no fold planned, a member standing outside the tree is the ground
    // this walk lands on: its rungs seed the run's map first
    // (`seed_member_rungs`), and the fold behind the walk asks the same
    // question of the same findings. An *as new* run owes a tree of its own,
    // so the member's shelves are not seats it files onto.
    if !quiet && plan.fold.is_none() && plan.rename.is_none() {
        seed_member_rungs(state, &mut folder, &found);
    }

    let mut walk = plan_the_walk(
        state,
        &mut folder,
        &mut books,
        &mut found,
        asked,
        &plan,
        quiet,
    );

    if walk.adds.is_empty()
        && walk.relinked == 0
        && walk.healed == 0
        && walk.asks.is_empty()
        && walk.replacements.is_empty()
        && walk.represented.is_empty()
    {
        // Nothing to do. A quiet run leaves no trace beyond the folder's
        // "last scanned" stamp; an explicit import still owes the reader a
        // card saying nothing was new — and a covered re-pick owes the note
        // as well.
        let stamped = now_ms();
        folder.scanned_ms = stamped;
        // The books of a tree whose shape the reader just answered the other way are already in the
        // library, so a walk that found nothing new is still a walk that owes the re-shape.
        let mut reshaped = own_shape
            .as_deref()
            .and_then(|rung| reshape_the_tree(state, &mut folder, rung, shaped, stamped));
        let root_rung = folder.shelf_map.get("").cloned();
        let folder_id = folder.id.clone();
        write_folder(state, folder);
        if !quiet {
            let folded = state
                .library
                .folder(&folder_id)
                .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found));
            // A fold answers the shape for the tree it takes the pick into: the books it spread
            // across that tree's one shelf come back to the rungs their own addresses name.
            if folded.is_some() {
                reshaped = fold_shape
                    .as_ref()
                    .and_then(|(row, rung)| reshape_row(state, row, rung, shaped, stamped))
                    .or(reshaped);
            }
            match folded {
                Some((shelf_id, name)) => {
                    conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
                }
                None => match reshaped.as_deref() {
                    Some(seat) => reveal::reveal_shelf(state, seat),
                    None => {
                        if let Some(Continuation { shelf_id, name }) = plan.continuation.clone() {
                            conflict::raise_note(state, shelf_id, name, NoteKind::NothingNew);
                        }
                    }
                },
            }
            update_task(state, &task, |t| t.finish());
        }
        // A quiet run's stamp is not worth a write; a re-shape is — the folder row now carries
        // the map the re-shape moved, and a restart still finds the shape the reader answered for.
        if reshaped.is_some() {
            crate::storage::persist_library(state.library);
        }
        return;
    }

    let now = now_ms();
    // Minted off the crate's own counter rather than the snapshot's length: two watched folders
    // rescan concurrently, and two tasks that both counted the library as it was BEFORE their walks
    // would mint the same id twice.
    let adds = std::mem::take(&mut walk.adds);
    let pending: Vec<(String, &FoundFile)> =
        adds.iter().map(|file| (id::next_id(now), file)).collect();
    let replaced = walk.replacements.len();
    let expected = run_total(
        pending.len() as u32,
        walk.relinked + walk.healed + replaced,
        0,
    );
    if quiet {
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

    // The run's facts in one value, the copy set moving out of the walk it was planned in:
    // the landing takes the walk by `&mut`, and the mints read the set through the value.
    // Each copy already carries its own measurement home — an independent copy of a file a
    // tree still reads in place never wears the ORIGINAL's fingerprint, and no second pass
    // over the stores is owed.
    let landing = Landing {
        copies: &copies,
        copy_paths: std::mem::take(&mut walk.copy_paths),
        planned_name: &plan.rename,
        root: &root,
        mode: folder.mode(),
        merged: plan.into.is_some(),
        now,
    };
    let tally = land_the_walk(state, &mut folder, &mut walk, pending, &landing);

    // A shape the reader answered the other way re-files the books the landing left where they
    // stood: what the walk brings home is the files that are new, not the tree already here. A
    // shape answered for a tree this run only picks a rung of waits for the fold below, which is
    // what puts the pick's ground inside that tree first.
    let mut reshaped = own_shape
        .as_deref()
        .and_then(|rung| reshape_the_tree(state, &mut folder, rung, shaped, now));

    folder.scanned_ms = now;
    let root_rung = folder.shelf_map.get("").cloned();
    let folder_id = folder.id.clone();
    write_folder(state, folder);
    // Hung in the run that finished the tree rather than at the start of the next one: the re-hang
    // at the top of this function read the map the walk was about to write.
    rehang(state, &folder_id);
    crate::storage::persist_library(state.library);
    covers::backfill_missing(state);

    // Runs before any light, so what is revealed is the shelf WHERE IT NOW IS.
    let folded = if quiet {
        None
    } else {
        state
            .library
            .folder(&folder_id)
            .and_then(|folder| run_fold(state, &plan, &folder, root_rung.as_deref(), &found))
    };
    // The tree the fold took the pick into is the row the shape answer was about: the books it
    // spread across its root rung come back to the rungs their own addresses name.
    if folded.is_some() {
        reshaped = fold_shape
            .as_ref()
            .and_then(|(row, rung)| reshape_row(state, row, rung, shaped, now))
            .or(reshaped);
    }

    // What a FOLDER import reveals is the folder: the shelf the reader's pick named, lit on the
    // level that holds it — the reader stays outside, where the folder is visible. For a re-pick
    // that is the RUNG, because the rung is the ground the reader asked about.
    if !quiet {
        match folded {
            Some((shelf_id, name)) => {
                conflict::raise_note(state, shelf_id, name, NoteKind::Returned)
            }
            None => {
                let light = plan
                    .continuation
                    .as_ref()
                    .map(|Continuation { shelf_id, .. }| shelf_id.clone())
                    .or(reshaped.clone())
                    .or_else(|| root_rung.clone());
                if let Some(shelf_id) = light {
                    reveal::reveal_shelf(state, &shelf_id);
                }
            }
        }
    }

    let waiting = walk.asks.len() as u32;
    if !walk.asks.is_empty() {
        let asks = std::mem::take(&mut walk.asks);
        conflict::raise(state, asks);
    }

    let healed = walk.healed + tally.healed;
    // The same arithmetic the expected count came from, represented rows included at the
    // finish where the reader is told what the run lit up (see `run_total`).
    let total = run_total(
        tally.placed,
        tally.relinked + healed + replaced,
        walk.represented.len(),
    );
    finish_task(state, &task, total, waiting);
}

/// Put the folder row back. One place, because the ledger is the part of the
/// library that must never be written half-updated: a `placed` set that lost an
/// entry re-adds a book the reader already filed. Written through `update` for the
/// same reason the books are — two imports running at once must not each replace
/// the other's folder row.
pub(super) fn write_folder(state: AppState, folder: WatchedFolder) {
    state.library.folders.update(
        |folders| match folders.iter().position(|f| f.id == folder.id) {
            Some(at) => folders[at] = folder,
            None => folders.push(folder),
        },
    );
}
