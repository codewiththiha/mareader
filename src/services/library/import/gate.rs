//! The read-at-place gate in front of a folder import: the family questions a pick of
//! ground answers BEFORE any sheet or walk. The stored arrival asks the family nothing
//! and goes straight to the level's own name question.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{self as folder_ops, rel_under, FolderOpts, WatchedFolder};
use library_core::governance::Governance;
use library_core::id;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};

use super::claim::{already_importing, claim_root, root_is_claimed, when_root_is_free};
use super::folder::run_folder;
use super::tasks::{finish_task, push_task, task_id};
use super::{rel_of, root_shelf_of, shelf_name, Asked};
use crate::services::library::conflict;
use crate::services::library::folder_label;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// What the folder sheet's answer decided about the run's root, before the run. The default
/// is the run nobody asked about: mint the folder's root shelf under the folder's own name.
/// A collision at the level changes that, and the change is a value rather than a branch at
/// six call sites.
#[derive(Clone, Default)]
pub(crate) struct RootPlan {
    pub rename: Option<String>,
    pub into: Option<String>,
    pub continuation: Option<(String, String)>,
    pub fold: Option<(String, String)>,
}

/// The dock owns the feedback from here on. A folder whose NAME the root level already
/// holds is a question before it is an import — two shelves of one name on one level are two
/// doors a reader cannot tell apart. The question goes to the family gate first.
pub fn import_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    watch: Option<GroundWatch>,
) {
    if let Some(watch) = watch {
        set_rung_tracking(state, &watch);
    }
    if opts.mode().reads_in_place() {
        if let Some(covered) = covered_shelf(state, &root) {
            // The walk runs on the reader's ask, on the TREE's ledger and root, so a rung cannot mint a
            // second instance of itself and the books a removal logged come back wherever in the tree
            // they stood. The continuation names the shelf the pick meant.
            let folders = state.library.folders.get_untracked();
            let fold = folders
                .iter()
                .find(|f| f.root == covered.tree_root)
                .and_then(|f| {
                    let shelves = state.library.shelves.get_untracked();
                    shelves_ops::family_for(&folders, &shelves, &f.root)
                });
            proceed_folder(
                state,
                covered.tree_root,
                opts,
                RootPlan {
                    continuation: Some((covered.shelf_id, covered.shelf_name)),
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
        // Not covered, but the ground may still be a family's: the rung the pick names was deleted,
        // or departed as a copy. An import of a folder is the reader wanting it BACK, and back is the
        // rung its directory names in the tree that covers it.
        let fold = {
            let folders = state.library.folders.get_untracked();
            let shelves = state.library.shelves.get_untracked();
            shelves_ops::family_for(&folders, &shelves, &root)
        };
        if fold.is_some() {
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
    }
    let incoming = folder_label(&root);
    let shelves = state.library.shelves.get_untracked();
    if let Some(existing_id) = library_core::conflict::collide_shelf(&shelves, None, &incoming) {
        let own = state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .find(|f| f.root == root)
                .and_then(|f| f.shelf_map.get("").cloned())
                == Some(existing_id.clone())
        });
        let existing_name = shelves_ops::find(&shelves, &existing_id)
            .map(|s| s.name.clone())
            .unwrap_or_else(|| incoming.clone());
        conflict::raise_shelf(
            state,
            conflict::ShelfConflictAsk {
                incoming_name: incoming,
                existing_id,
                existing_name,
                root,
                opts,
                own,
            },
        );
        return;
    }
    proceed_folder(state, root, opts, RootPlan::default());
}

/// A value rather than a tuple at the call site: the gate's covered branch reconciles on `tree_root` and lights `shelf_id`.
pub(super) struct Covered {
    pub shelf_id: String,
    pub shelf_name: String,
    /// The ledger row of the tree that covers the ground. A tracking decision is written at THIS
    /// and never at `tree_root`, which is the directory the run reconciles: a row is found by id.
    pub tree_id: String,
    pub tree_root: String,
    /// `""` when the ground IS the tree's root, which is what makes the sheet's switch a decision about this rung rather than about the whole import.
    pub rel: String,
}

/// The sheet's tracking switch as the write it becomes: the tree covering the ground the reader
/// picked, the rung that ground IS or stands on in it, and the answer itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroundWatch {
    pub tree_id: String,
    pub rung: String,
    pub on: bool,
}

/// The rung a pick of `root` stands on when a read-at-place tree already covers it, and the
/// decision that rung carries now — the state the sheet's switch opens seeded with. `None` when no
/// tree covers the ground: nothing is tracking it, so the switch writes the landing folder's own
/// root.
pub fn ground_tracking(state: AppState, root: &str) -> Option<GroundWatch> {
    let covered = covered_shelf(state, root)?;
    let on = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &covered.tree_id)
            .map(|folder| folder.tracks_rung(&covered.rel))
            .unwrap_or(false)
    });
    Some(GroundWatch {
        tree_id: covered.tree_id,
        rung: covered.rel,
        on,
    })
}

/// The write the sheet owes when a tree covers the ground its switch answered about: the rung the
/// pick names rather than the tree's root, because the tree's other rungs keep their own answers.
/// Answers whether the tree's decision changed.
pub(super) fn write_rung_tracking(folders: &mut [WatchedFolder], watch: &GroundWatch) -> bool {
    let Some(folder) = folder_ops::find_mut(folders, &watch.tree_id) else {
        return false;
    };
    if folder.tracks_rung(&watch.rung) == watch.on {
        return false;
    }
    folder.set_tracking(&watch.rung, watch.on);
    true
}

/// The walk a new decision owes is the import's own: every door into [`import_folder`] goes on to a
/// run on that ground, which reads the tree as this write left it — so the write owes no walk of
/// its own.
fn set_rung_tracking(state: AppState, watch: &GroundWatch) {
    let mut changed = false;
    state.library.folders.update(|folders| {
        changed = write_rung_tracking(folders, watch);
    });
    if changed {
        crate::storage::persist_library(state.library);
    }
}

pub(super) fn covered_shelf(state: AppState, root: &str) -> Option<Covered> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    covered_of(&folders, &shelves, root)
}

/// A folder's own tree answers for it before a tree it stands inside — the empty rung wins —
/// because the folder's own root shelf is the door the reader meant. Only READ-AT-PLACE trees
/// answer here: their shelves are the OS folders themselves.
pub(super) fn covered_of(
    folders: &[WatchedFolder],
    shelves: &[Shelf],
    root: &str,
) -> Option<Covered> {
    // Both lookups below are guaranteed to land, so the `?` is a shape rather than a second rule.
    let coverage = Governance::new(folders, shelves).covering(root)?;
    let shelf = shelves_ops::find(shelves, &coverage.shelf_id)?;
    let folder = folder_ops::find(folders, &coverage.folder_id)?;
    Some(Covered {
        shelf_name: shelf.name.clone(),
        tree_id: coverage.folder_id,
        tree_root: folder.root.clone(),
        rel: coverage.rel,
        shelf_id: coverage.shelf_id,
    })
}

/// The shape this answers, and it is one the reader makes rather than a bug: a rung of a
/// watched tree is removed, which cuts the folder's pointer to it and leaves the folder
/// watching; that same subfolder is then imported on its own.
pub(super) fn displaced_member(
    state: AppState,
    folder: &WatchedFolder,
    found: &[FoundFile],
) -> Option<DisplacedMember> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let mut best: Option<(usize, DisplacedMember)> = None;
    for other in folders.iter() {
        if other.id == folder.id || other.mode().copies_files() {
            continue;
        }
        let Some(rel) = rel_under(&other.root, &folder.root).filter(|rel| !rel.is_empty()) else {
            continue;
        };
        if !found
            .iter()
            .any(|file| rel_under(&file.path, &other.root).is_some())
        {
            continue;
        }
        // By its map first and by its kind second: the map is what its walk files onto, and a map that lost the pointer still leaves a shelf the folder owns.
        let Some(shelf_id) = other
            .shelf_map
            .get("")
            .filter(|id| shelves.iter().any(|s| &s.id == *id))
            .cloned()
            .or_else(|| root_shelf_of(&shelves, &other.id))
        else {
            continue;
        };
        let Some(shelf) = shelves_ops::find(&shelves, &shelf_id) else {
            continue;
        };
        if shelf.kind.folder_id() == Some(folder.id.as_str())
            || hangs_inside(&shelves, &shelf_id, &folder.id)
        {
            continue;
        }
        let depth = rel.matches('/').count();
        if best.as_ref().is_none_or(|(seen, _)| depth < *seen) {
            best = Some((
                depth,
                DisplacedMember {
                    folder_id: other.id.clone(),
                    rel,
                    shelf_id,
                    shelf_name: shelf.name.clone(),
                },
            ));
        }
    }
    best.map(|(_, member)| member)
}

pub(super) struct DisplacedMember {
    pub(super) folder_id: String,
    pub(super) rel: String,
    pub(super) shelf_id: String,
    pub(super) shelf_name: String,
}

/// Bounded by the list rather than by the walk finding its own tail, so a blob that already carries a cycle answers "no" rather than spinning.
fn hangs_inside(shelves: &[Shelf], shelf_id: &str, folder_id: &str) -> bool {
    let mut current = shelves_ops::find(shelves, shelf_id).and_then(|s| s.parent.clone());
    for _ in 0..=shelves.len() {
        let Some(id) = current else {
            return false;
        };
        let Some(parent) = shelves_ops::find(shelves, &id) else {
            return false;
        };
        if parent.kind.folder_id() == Some(folder_id) {
            return true;
        }
        current = parent.parent.clone();
    }
    false
}

/// Put a displaced member back on the rung its directory names, and fold the folder that was
/// reading it into the tree that contains it: one ground is read by one folder from here on.
/// Three writes, in the order that keeps them honest.
pub(crate) fn reclaim_rung(
    state: AppState,
    tree_id: &str,
    gone_id: &str,
    rel: &str,
    shelf_id: &str,
) -> bool {
    let now = now_ms();
    let mut minted: Vec<Shelf> = Vec::new();
    let Some((mut tree, gone)) = state.library.folders.with_untracked(|folders| {
        let tree = folder_ops::find(folders, tree_id)?.clone();
        let gone = folder_ops::find(folders, gone_id)?.clone();
        Some((tree, gone))
    }) else {
        return false;
    };
    // Read before anything is written, because the answer is about the shelves that are standing and not about the ones this move is going to mint.
    let rungs: Vec<(String, String)> = state.library.shelves.with_untracked(|shelves| {
        shelves
            .iter()
            .filter(|s| s.kind.folder_id() == Some(gone_id))
            .filter_map(|s| {
                let ShelfKind::Folder { rel: own, .. } = &s.kind else {
                    return None;
                };
                let own = own.clone().unwrap_or_default();
                Some((
                    s.id.clone(),
                    if own.is_empty() {
                        rel.to_string()
                    } else {
                        format!("{rel}/{own}")
                    },
                ))
            })
            .collect()
    });
    // A shelf that went while the note was up is an answer with nothing to move; so is a tree being walked right now, whose run writes its clone of the ledger back whole.
    if !rungs.iter().any(|(id, _)| id == shelf_id) || root_is_claimed(&tree.root) {
        return false;
    }
    let root = tree.root.clone();
    let parent = tree.shelf_chain_for(
        library_core::folder::parent_key(rel).unwrap_or(""),
        |_| id::next_shelf_id(now),
        |rung| shelf_name(rung, &root),
        |rung, id, name, parent| {
            minted.push(Shelf {
                id: id.to_string(),
                name,
                kind: ShelfKind::Folder {
                    folder_id: tree_id.to_string(),
                    rel: rel_of(rung),
                },
                books: Vec::new(),
                parent,
                manual_parent: false,
            });
        },
    );
    for (id, key) in &rungs {
        tree.shelf_map.insert(key.clone(), id.clone());
    }
    // The answer the folded row carried for its own root becomes the rung it becomes: the row that
    // answer was written on is the one this fold retires.
    tree.set_tracking(rel, gone.opts.watch);
    tree.placed.extend(gone.placed.iter().copied());
    for stone in gone.ignored.iter() {
        if !tree.is_ignored(&stone.fp) {
            tree.ignored.push(stone.clone());
        }
    }
    tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);

    state.library.shelves.update(|shelves| {
        for shelf in minted {
            if !shelves.iter().any(|s| s.id == shelf.id) {
                shelves.push(shelf);
            }
        }
        let nestable = shelves_ops::can_nest(shelves, shelf_id, &parent);
        for (id, key) in &rungs {
            let Some(shelf) = shelves_ops::find_mut(shelves, id) else {
                continue;
            };
            shelf.kind = ShelfKind::Folder {
                folder_id: tree_id.to_string(),
                rel: rel_of(key),
            };
            if id != shelf_id {
                continue;
            }
            if nestable {
                shelf.parent = Some(parent.clone());
                shelf.manual_parent = false;
            } else {
                shelf.manual_parent = true;
            }
        }
    });
    state.library.folders.update(|folders| {
        folders.retain(|f| f.id != gone_id);
        match folders.iter().position(|f| f.id == tree_id) {
            Some(at) => folders[at] = tree,
            None => folders.push(tree),
        }
    });
    crate::storage::persist_library(state.library);
    true
}

/// The half of [`import_folder`] that is the same whatever the folder sheet decided, and the half its answers call directly.
pub(crate) fn proceed_folder(
    state: AppState,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
) {
    // A folder already being imported is an import already answering this ask: racing it would
    // clobber its ledger write. A RESCAN of the same tree is not refused — an ask outranks it.
    let task = task_id();
    let card = task.clone();
    let walking = root.clone();
    if !when_root_is_free(&root, move || {
        start_folder_run(state, walking, opts, plan, card)
    }) {
        already_importing(state, &root);
        return;
    }
    push_task(state, ImportTask::new(task, folder_label(&root)));
}

/// One shape for the two starts an import has, because the two owe the same run and the same card.
fn start_folder_run(
    state: AppState,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
    task: String,
) {
    let Some(claim) = claim_root(&root, Asked::Explicitly) else {
        // The single thread leaves no room for the race, but the card is already up, so it is closed rather than left counting a walk that never started.
        finish_task(state, &task, 0, 0);
        return;
    };
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::Explicitly, plan).await;
    });
}

/// The plan's own fold first: the run walked the picked folder's ledger, and its root shelf
/// is the member going home. With no fold planned, the run asks whether a member is standing
/// outside the tree it belongs to.
pub(super) fn run_fold(
    state: AppState,
    plan: &RootPlan,
    folder: &WatchedFolder,
    root_rung: Option<&str>,
    found: &[FoundFile],
) -> Option<(String, String)> {
    if let Some((tree, rel)) = &plan.fold {
        let rung = root_rung?;
        reclaim_rung(state, tree, &folder.id, rel, rung)
            .then(|| (rung.to_string(), state.library.shelf_name(rung)))
    } else {
        let member = displaced_member(state, folder, found)?;
        reclaim_rung(
            state,
            &folder.id,
            &member.folder_id,
            &member.rel,
            &member.shelf_id,
        )
        .then(|| (member.shelf_id.clone(), member.shelf_name.clone()))
    }
}
