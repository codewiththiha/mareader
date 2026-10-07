//! The folder run: scan, diff, copy, land; [`run_folder`] is the order.

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
use crate::services as ipc;
use crate::services::conflict::{self, ConflictAsk};
use crate::services::covers;
use crate::services::folder_label;
use crate::services::reveal;
use crate::state::library::{ImportTask, NoteKind};
use runtime_contract::time::now_ms;

/// One spelling for the two loops a run mints rungs through.
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
            // The *as new* answer: the root rung wears the counter name.
            (true, Some(name)) => name.clone(),
            _ => rung_label(rung, root),
        },
        |rung, id, name, parent| {
            let mut minted = Shelf::folder_shelf(id, name, &folder_id, rel_of(rung), parent);
            // The scan owns its rung until a hand moves it.
            minted.manual_parent = merged;
            new_shelves.push(minted);
        },
    )
}

/// The heal half of the migrated-row rule: the address answers.
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

/// One picture for the stages; nothing here is written back.
pub(super) struct Snapshot<'a> {
    pub(super) books: &'a [Row],
    pub(super) registry: &'a ledger::Registry,
    pub(super) found: &'a [FoundFile],
    pub(super) copy_paths: &'a HashSet<String>,
}

impl Snapshot<'_> {
    /// The row answering for a file: identity first, then address.
    fn known_row(&self, file: &FoundFile) -> Option<String> {
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

/// A re-import continues the row's `placed` and `ignored` sets.
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
    // A continuation re-pick writes the sheet's answers onto its rung.
    let root_shape = folder.shape_at("");
    folder.opts = opts;
    // The sheet's switch asks the root rung; the tree mirrors it.
    if folder.mode().reads_in_place() {
        if plan.continuation.is_none() {
            folder.set_tracking("", folder.opts.watch);
        } else {
            folder.opts.watch = folder.tracking.tracked();
            folder.opts.groups = root_shape;
        }
    }
    // Cut before the map is written, so the answers read what is left.
    folder.prune_shelf_map(shelves);
    // A merge files into the shelf the level already held.
    if let Some(into) = &plan.into {
        folder.shelf_map.insert(String::new(), into.clone());
    }
    // An *as new* answer owes a tree of its own: rungs minted fresh.
    if plan.rename.is_some() {
        folder.shelf_map.clear();
    }
    folder
}

/// Ask `rehang_moves` and apply it, before the walk's placements.
fn rehang(state: crate::context::LibraryContext, folder_id: &str) {
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
    crate::services::persist_library(state.library);
}

/// A planned tree places a file's row rather than mints a second one.
fn planned_placements(
    state: crate::context::LibraryContext,
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

/// [`planned_placements`]' question for the files the library lacks.
fn screen_merge_adds(
    state: crate::context::LibraryContext,
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

/// The merge's question about one file: its seat, and its name.
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

/// The merge half of a re-pick, which no ledger table answers.
pub(super) fn returned_memberships(
    state: crate::context::LibraryContext,
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
        // The shelf module says where a book is; the kind says whose.
        let on_the_tree = shelves_ops::containing(&shelves_now, &row_id)
            .iter()
            .any(|shelf| shelf.kind.folder_id() == Some(folder_id));
        if !on_the_tree {
            out.push((row_id, file.clone()));
        }
    }
    out
}

/// One value for the run's facts, rather than eight arguments.
pub(super) struct Landing<'a> {
    /// The copies that came home, each with its measurement.
    pub(super) copies: &'a HashMap<String, Landed>,
    /// Owned: the landing takes the walk by `&mut`.
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

/// What a mints report: placed, healed, or a copy that failed.
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
    // A copy-list file becomes a book of its own, independent.
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
    // The whole chain, not the leaf: "1" holds "2".
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

/// The diff stage: the walk's findings to the copy batch.
fn plan_the_walk(
    state: crate::context::LibraryContext,
    folder: &mut WatchedFolder,
    books: &mut Vec<Row>,
    found: &mut Vec<FoundFile>,
    asked: Asked,
    plan: &RootPlan,
    quiet: bool,
) -> WalkPlan {
    let registry = ledger::registry_of(books);

    // Addresses the library reads in place, where this run copies.
    let copy_paths: HashSet<String> = if folder.mode().copies_files() && !quiet {
        ledger::copy_over_paths(found, &registry, books)
    } else {
        HashSet::new()
    };

    ledger::prune_tombstones(folder, &registry);
    // Written on every scan, quiet ones included.
    folder.record_seen(found);

    let represented: Vec<String> = if quiet {
        Vec::new()
    } else {
        take_represented(state, Some(&folder.id), found)
    };

    let mut adds: Vec<FoundFile> = Vec::new();
    let mut relinks: Vec<(String, String)> = Vec::new();
    // Two tables, one question each: focus, then explicit.
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

    // Copy-run files were skipped as known; each still owes a book.
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

    // An address the library holds is that book, fingerprints aside.
    let healed_paths = heal_by_address(books, &mut adds, &copy_paths);
    let healed = healed_paths.len();

    // One book per fingerprint per scan: no orphan copy in the store.
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
        // A removal after this gets a tombstone: a skip needs a book.
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

/// The shelves a mint reported, added unless already there.
pub(super) fn page_shelves(state: crate::context::LibraryContext, minted: Vec<Shelf>) {
    state
        .library
        .shelves
        .update(|shelves| page_into(shelves, minted));
}

/// [`page_shelves`] for a caller already inside a shelf write.
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

/// The diff written to the live lists, not the snapshot.
fn land_the_walk(
    state: crate::context::LibraryContext,
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
                    // What a removal kept lands with the file that came back.
                    kept::reclaim(books, file, &id);
                    placements.push((id, shelf));
                    placed += 1;
                }
                Minted::Healed => healed_here += 1,
                Minted::CopyFailed => {}
            }
        }
        // The chain mints missing rungs; a book already placed stays.
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

/// The stages in order; [`run_folder`] is their one caller.
pub(super) async fn run_folder(
    state: crate::context::LibraryContext,
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

    // The shape question is about a row and a rung.
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
    // The answer is written before the walk, so the fold reads it.
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
    // A standing member's rungs seed the run's map first.
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
        // Nothing to do: the stamp, then whatever the sheet still owes.
        let stamped = now_ms();
        folder.scanned_ms = stamped;
        // A shape answered the other way still owes its re-shape.
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
            // A fold answers the shape for the tree it takes the pick into.
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
        // A quiet stamp is not worth a write; a re-shape is.
        if reshaped.is_some() {
            crate::services::persist_library(state.library);
        }
        return;
    }

    let now = now_ms();
    // Ids minted off the crate's counter, never the snapshot's length.
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

    // The run's facts in one value, with the copy set moved out.
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

    // A shape answered the other way re-files the books already here.
    let mut reshaped = own_shape
        .as_deref()
        .and_then(|rung| reshape_the_tree(state, &mut folder, rung, shaped, now));

    folder.scanned_ms = now;
    let root_rung = folder.shelf_map.get("").cloned();
    let folder_id = folder.id.clone();
    write_folder(state, folder);
    // Hung here, not at the start: the re-hang read the walk's map.
    rehang(state, &folder_id);
    crate::services::persist_library(state.library);
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
    // The folded tree's books come back to their own rungs.
    if folded.is_some() {
        reshaped = fold_shape
            .as_ref()
            .and_then(|(row, rung)| reshape_row(state, row, rung, shaped, now))
            .or(reshaped);
    }

    // What a folder import reveals is the shelf the pick named.
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
    // The same arithmetic as the expected count, represented rows in.
    let total = run_total(
        tally.placed,
        tally.relinked + healed + replaced,
        walk.represented.len(),
    );
    finish_task(state, &task, total, waiting);
}

/// Put the folder row back; the ledger is never written half-updated.
pub(super) fn write_folder(state: crate::context::LibraryContext, folder: WatchedFolder) {
    state.library.folders.update(
        |folders| match folders.iter().position(|f| f.id == folder.id) {
            Some(at) => folders[at] = folder,
            None => folders.push(folder),
        },
    );
}
