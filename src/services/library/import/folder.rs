//! The folder run: scan one watched folder, run the ledger over what the walk found,
//! copy whatever the options say to copy, and write the result in one go. The stages
//! are the functions below; [`run_folder`] is the order they run in.

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
use super::gate::{run_fold, seed_member_rungs, RootPlan};
use super::kept;
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

/// One spelling for the two loops a folder run mints through — the books it adds and the
/// books the library already held, which a planned tree owes a membership of — so a rung
/// cannot be minted twice under two spellings of its own name.
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
            // The folder sheet's *as new* answer: the root rung wears the counter name it promised.
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
                // Minted by the scan, so the scan owns its rung — until a hand moves it, which is `reparent`'s mark.
                manual_parent: merged,
            });
        },
    )
}


/// The heal half of the migrated-row rule: a file at an address the library reads IS that
/// book, whatever the two fingerprints say.
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

/// A value so the stages below read one consistent picture rather than each taking its own
/// borrow of four locals. Nothing in it is written back: what lands is applied to the LIVE
/// signals at the end.
pub(super) struct Snapshot<'a> {
    pub(super) books: &'a [Row],
    pub(super) registry: &'a ledger::Registry,
    pub(super) found: &'a [FoundFile],
    pub(super) copy_paths: &'a HashSet<String>,
}

/// Importing a folder the library already watches continues that row's `placed` and `ignored`
/// sets, which is the whole point of them: re-importing is how a reader would otherwise get
/// back every book they deleted last week.
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
    // A CONTINUATION of a standing tree is a re-pick of ground the tree already covers, and the
    // sheet's switch on covered ground answered for the RUNG the pick names — an answer
    // `import_folder` wrote on that rung before this run started. The tree's root is not this
    // run's to answer for.
    folder.opts = opts;
    // The sheet's switch is a question about the ROOT RUNG, and the tree is what answers it from
    // here on. The two stay agreed because `set_tracking` mirrors the root's answer back onto the
    // flag the sheet and the older surfaces read.
    if folder.mode().reads_in_place() {
        if plan.continuation.is_none() {
            folder.set_tracking("", folder.opts.watch);
        } else {
            folder.opts.watch = folder.tracking.tracked();
        }
    }
    // Cut before the map is written to below, so a merge's root and an *as new* run's clearing are answers about the map that is left.
    folder.prune_shelf_map(shelves);
    // A merge files into the shelf the level already held: the folder's root rung is that shelf, which is what makes the merge a promise the next scan keeps.
    if let Some(into) = &plan.into {
        folder.shelf_map.insert(String::new(), into.clone());
    }
    // An *as new* answer owes a tree of its OWN: every rung is minted fresh under the counter-named root.
    if plan.rename.is_some() {
        folder.shelf_map.clear();
    }
    folder
}

/// The rule and its edge cases are `library_core::shelf::rehang_moves`', pure and
/// host-tested; this is the one pass that asks it and applies the answer. Runs before the
/// walk, so the placements below land on the tree as it now stands.
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

/// A planned tree — the folder sheet's *as new* or *merge* answer — owes a placement for
/// EVERY file the walk found that the library already holds: a membership of the row it
/// holds it in, never a second row, because one content is one identity.
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
    adds.retain(|f| {
        !snap.registry.contains_key(&f.fp) || snap.copy_paths.contains(&f.path)
    });
    let shelves_now = state.library.shelves.get_untracked();
    let mut replacements = Vec::new();
    let mut asks = Vec::new();
    for file in snap.found {
        // By content identity first (the ledger's own answer), and by address second for the migrated row whose placeholder identity no measurement matched.
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

/// The same question [`planned_placements`] asks of the files the library already held,
/// asked of the ones it did not: a merge's new arrivals land on a rung that may already hold
/// their NAME, and a collision there is the compact sheet's too.
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

/// The merge half of a re-pick, and the half the ledger's table cannot answer. Two readers
/// make this shape, and neither is a tombstone: a book the reader filed onto a shelf of their
/// own, and a merge that took the rung a file used to sit on.
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
        // Which shelves a book is on is the shelf module's question, and which of them this folder owns is the shelf KIND's.
        let on_the_tree = shelves_ops::containing(&shelves_now, &row_id)
            .iter()
            .any(|shelf| shelf.kind.folder_id() == Some(folder_id));
        if !on_the_tree {
            out.push((row_id, file.clone()));
        }
    }
    out
}

/// A value rather than eight arguments: every one of them is a fact about the RUN and not about the file.
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

pub(super) enum Minted {
    Placed { id: String, shelf: String },
    Healed,
    CopyFailed,
}

/// The three outcomes are the three things that can be true of a file a walk found, and
/// telling them apart is the run's whole job: a row already reads this address, the file is
/// new, or the copy the options owed did not land.
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
    // The copy list's file is a book of its own beside the linked book the tree keeps:
    // independent, so its marks and its place in it are its own. `add_book`'s one-row-per-
    // fingerprint rule is the right rule for a walk and the wrong one for the second instance
    // the reader just asked for.
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
    // The whole chain, not the leaf: importing "1" whose inside is "2" and four books has to produce "1" at the root with them inside it.
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

/// One value, so the stages after the diff read one answer rather than eight locals.
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

/// The diff stage: everything between the walk's raw findings and the copy batch. Decides
/// against the SNAPSHOT; the only thing it writes is the folder's own ledger row.
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

    // The addresses whose file the library already reads IN PLACE, and where this run lands a
    // copy of its own beside the linked row: a copies import is the library's own second
    // instance, unrelated to the tree that reads the ground.
    let copy_paths: HashSet<String> = if folder.mode().copies_files() && !quiet {
        ledger::copy_over_paths(found, &registry, books)
    } else {
        HashSet::new()
    };

    ledger::prune_tombstones(folder, &registry);
    // Written on every scan, including one that changes nothing: a walk that found nothing to do still saw every file.
    folder.record_seen(found);

    let represented: Vec<String> = if quiet {
        Vec::new()
    } else {
        take_represented(state, Some(&folder.id), found)
    };

    let mut adds: Vec<FoundFile> = Vec::new();
    let mut relinks: Vec<(String, String)> = Vec::new();
    // Two tables, one question each: what should come back on its own, and what the reader is asking for right now.
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

    // The ledger answered Skip for the copy run's own files — their content is known — but the run owes each a book of its own.
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

    let (mut replacements, mut asks) = {
        let snap = Snapshot {
            books,
            registry: &registry,
            found,
            copy_paths: &copy_paths,
        };
        planned_placements(state, &snap, folder, plan, &mut adds)
    };

    // A file at an address the library already holds IS that book, whatever the two fingerprints
    // say: the row this catches is one migrated from the previous schema, carrying a placeholder
    // identity nothing ever measured.
    let healed_paths = heal_by_address(books, &mut adds, &copy_paths);
    let healed = healed_paths.len();

    // One book per fingerprint inside a single scan: a tree holding two byte-identical files is
    // one book, and copying both would leave an orphan in the store that nothing can ever remove.
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

    // Asked last, once `adds` is the list that is actually going to be minted.
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
        // A book removed after this is a removal THIS folder takes a tombstone for, and a fingerprint
        // the ledger skips with no book behind it is the one state a folder cannot recover from.
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

struct LandTally {
    placed: u32,
    relinked: usize,
    healed: usize,
}

/// The diff's answer, written to the LIVE lists rather than to the snapshot: a walk of a big
/// folder takes seconds, and a reader who opens a book during one must not have that read
/// overwritten by the write at the end.
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
    let mut placed = 0u32;
    let mut relink_count = 0usize;
    let mut healed_here = 0usize;
    let mut new_shelves: Vec<Shelf> = Vec::new();
    let mut placements: Vec<(String, String)> = Vec::new();
    let mode = folder.mode();
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
                    // A file that came back is the file that left: what was kept lands with it.
                    kept::reclaim(books, file, &id);
                    placements.push((id, shelf));
                    placed += 1;
                }
                Minted::Healed => healed_here += 1,
                Minted::CopyFailed => {}
            }
        }
        // The chain mints whatever rungs are not in the map yet, and the member guard below keeps a
        // book that is already where it is being put from moving to the end of it.
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

/// The order the stages run in, rather than any of them: [`resolve_folder`], [`rehang`],
/// [`plan_the_walk`], the copy batch, [`land`], then the questions a merge asks.
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
    let mut found = match wire::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, fail_mode),
    };

    let mut books = state.library.books.get_untracked();
    let folders = state.library.folders.get_untracked();
    let shelves_now = state.library.shelves.get_untracked();

    let mut folder = resolve_folder(&folders, &shelves_now, &root, opts, &plan);
    rehang(state, &folder.id);
    // With no fold planned, a member standing outside the tree is the ground this walk is about to
    // land on: its rungs go into the run's map first (`seed_member_rungs`), and the fold behind the
    // walk is the same question asked of the same findings.
    if !quiet && plan.fold.is_none() {
        seed_member_rungs(state, &mut folder, &found);
    }

    let mut walk = plan_the_walk(state, &mut folder, &mut books, &mut found, asked, &plan, quiet);

    if walk.adds.is_empty()
        && walk.relinked == 0
        && walk.healed == 0
        && walk.asks.is_empty()
        && walk.replacements.is_empty()
        && walk.represented.is_empty()
    {
        // Nothing to do. A quiet run leaves no trace beyond the folder's own "last scanned" stamp;
        // an explicit import still owes the reader an answer, which is a card saying nothing was new —
        // and a re-pick of ground the library already reads in place owes the note as well.
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
    // Minted off the crate's own counter rather than the snapshot's length: two watched folders
    // rescan concurrently, and two tasks that both counted the library as it was BEFORE their walks
    // would mint the same id twice.
    let adds = std::mem::take(&mut walk.adds);
    let pending: Vec<(String, &FoundFile)> = adds
        .iter()
        .map(|file| (id::next_id(now), file))
        .collect();
    let replaced = walk.replacements.len();
    let expected = (pending.len() + walk.relinked + walk.healed + replaced) as u32;
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
    // The copy run's copies are measured in one pass before a row is promised: an independent copy
    // of a file a tree still reads in place must not wear the ORIGINAL's fingerprint.
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

    // What a FOLDER import reveals is the folder: the shelf the reader's pick named, lit on the
    // level that holds it — the reader stays outside, where the folder is visible. For a re-pick
    // that is the RUNG, because the rung is the ground the reader asked about.
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
