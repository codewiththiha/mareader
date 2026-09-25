//! The read-at-place gate in front of a folder import: the family questions
//! a pick of ground answers before any sheet or walk. A stored arrival asks
//! the family nothing and goes straight to the level's name question.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{self as folder_ops, FolderOpts, WatchedFolder, rel_under};
use library_core::governance::Governance;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops, Shelf, ShelfKind};

use super::claim::{claim_root, root_is_claimed, start_guarded};
use super::folder::{chain_for, page_into, page_shelves, run_folder};
use super::reshape::flatten_rungs;
use super::tasks::finish_task;
use super::{Asked, rel_of, root_shelf_of};
use crate::services::library::conflict;
use crate::services::library::folder_label;
use crate::state::AppState;
use crate::time::now_ms;

/// The shelf a run continues onto when the gate already knows it: a re-pick
/// of ground a tree covers lights the shelf the pick landed on. The default
/// run mints the folder's root shelf under the folder's own name; a collision
/// or a cover changes that through this value rather than a branch at six
/// call sites.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Continuation {
    pub shelf_id: String,
    pub name: String,
}

/// A pick folded into the tree that contains it: the tree's ledger row, and the rung the
/// pick's directory names inside it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Fold {
    pub tree_id: String,
    pub rel: String,
}

#[derive(Clone, Default)]
pub(crate) struct RootPlan {
    pub rename: Option<String>,
    pub into: Option<String>,
    pub continuation: Option<Continuation>,
    pub fold: Option<Fold>,
    /// The rung of this run's tree that the reader's pick answered for — the
    /// ground the sheet's answers stand for. `""` when the pick is the tree's
    /// own ground; a fold's rung takes over when there is one.
    pub rung: String,
}

impl RootPlan {
    /// The rung the pick answered for: the fold's rung when the pick folds
    /// into a tree, else the ground the gate read off the pick itself.
    pub(super) fn answered_rung(&self) -> String {
        match &self.fold {
            Some(Fold { rel, .. }) => rel.clone(),
            None => self.rung.clone(),
        }
    }
}

/// The dock owns the feedback from here on. A folder whose name the root
/// level already holds is a question before it is an import — two shelves of
/// one name are two doors a reader cannot tell apart — and a read-at-place
/// pick answers the family questions first.
pub fn import_folder(state: AppState, root: String, opts: FolderOpts, watch: Option<GroundWatch>) {
    if let Some(watch) = watch {
        set_rung_tracking(state, &watch);
    }
    if opts.mode().reads_in_place() {
        if let Some(covered) = covered_shelf(state, &root) {
            // The walk runs on the tree's ledger and root, so a rung cannot
            // mint a second instance of itself and removed books come back
            // wherever in the tree they stood. The continuation names the
            // shelf the pick meant.
            let folders = state.library.folders.get_untracked();
            let fold = folders
                .iter()
                .find(|f| f.root == covered.tree_root)
                .and_then(|f| {
                    let shelves = state.library.shelves.get_untracked();
                    shelves_ops::family_for(&folders, &shelves, &f.root)
                })
                .map(|(tree_id, rel)| Fold { tree_id, rel });
            proceed_folder(
                state,
                covered.tree_root,
                opts,
                RootPlan {
                    continuation: Some(Continuation {
                        shelf_id: covered.shelf_id,
                        name: covered.shelf_name,
                    }),
                    rung: covered.rel,
                    fold,
                    ..Default::default()
                },
            );
            return;
        }
        // Not covered, but the ground may still be a family's: the rung the
        // pick names was deleted or departed as a copy. An import of a folder
        // is the reader wanting it back, and back is the rung its directory
        // names in the tree that covers it — while that tree's root shelf
        // stands; a pick under a taken-out tree is a tree of its own.
        let fold = {
            let folders = state.library.folders.get_untracked();
            let shelves = state.library.shelves.get_untracked();
            shelves_ops::family_for(&folders, &shelves, &root)
        };
        let fold = fold.map(|(tree_id, rel)| Fold { tree_id, rel });
        if let Some(fold) = fold {
            proceed_folder(
                state,
                root,
                opts,
                RootPlan {
                    fold: Some(fold),
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

/// A named value rather than a tuple: the covered branch reconciles on
/// `tree_root` and lights `shelf_id`.
pub(super) struct Covered {
    pub shelf_id: String,
    pub shelf_name: String,
    /// The ledger row of the tree that covers the ground. Tracking is written
    /// at this id, never at `tree_root` — that is the directory the run
    /// reconciles, and a row is found by id.
    pub tree_id: String,
    pub tree_root: String,
    /// `""` when the ground is the tree's root, which makes the sheet's
    /// switch a decision about this rung rather than the whole import.
    pub rel: String,
}

/// What ground the library already reads answers the import sheet with: the
/// covering tree, the rung the ground is or stands on, that rung's tracking,
/// and the options the row was imported with (shape included) — so the sheet
/// opens on the folder's state rather than the last import's answers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroundWatch {
    pub tree_id: String,
    pub rung: String,
    pub on: bool,
    pub opts: FolderOpts,
}

/// The rung a pick of `root` answers for and its current tracking decision —
/// the state the sheet's switch opens seeded with. A tree whose shelf stands
/// on the ground answers with that rung; a tree that has not walked it in yet
/// answers with the rung the directory will fold onto. `None` when no ledger
/// row answers for the ground: nothing tracks it, so the switch writes the
/// landing folder's own root.
pub fn ground_tracking(state: AppState, root: &str) -> Option<GroundWatch> {
    // One snapshot answers both, so the rung is never read off a tree list
    // the family answer missed.
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let (tree_id, rung) = match covered_of(&folders, &shelves, root) {
        Some(covered) => (covered.tree_id, covered.rel),
        None => shelves_ops::family_for(&folders, &shelves, root)?,
    };
    let row = folder_ops::find(&folders, &tree_id)?;
    // The sheet opens on the row's answers except the shape: the structure
    // question belongs to the picked ground, and a rung a re-import moved
    // answers with the shape it was moved into.
    let mut opts = row.opts.clone();
    opts.groups = row.shape_at(&rung);
    Some(GroundWatch {
        on: row.tracks_rung(&rung),
        opts,
        tree_id,
        rung,
    })
}

/// The write the sheet owes when a tree covers the ground its switch answered
/// about — at the rung the pick names, not the tree's root, so the other
/// rungs keep their own answers. Answers whether the decision changed.
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

/// The walk a new decision owes is the import's own: every door into
/// [`import_folder`] goes on to a run that reads the tree as this write left
/// it, so the write owes no walk of its own.
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

/// A folder's own tree answers for it before a tree it stands inside — the
/// empty rung wins — because its own root shelf is the door the reader meant.
/// Only read-at-place trees answer here: their shelves are the OS folders
/// themselves.
pub(super) fn covered_of(
    folders: &[WatchedFolder],
    shelves: &[Shelf],
    root: &str,
) -> Option<Covered> {
    // Both lookups are guaranteed to land; the `?` is a shape, not a second
    // rule.
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

/// The member standing outside the tree: a read-at-place folder inside
/// `folder`'s ground, made by importing that subfolder on its own — a rung
/// removed first, or the subfolder picked before the folder above it. Only
/// while the shelf the member's row is kept on still stands: a shelf that
/// went is an answer with nothing to move.
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
        // By its map first, by its kind second: the map is what its walk
        // files onto, and a map that lost the pointer still leaves a shelf
        // the folder owns.
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
}

/// Bounded by the list rather than by the walk finding its own tail, so a
/// blob carrying a cycle answers "no" rather than spinning. The chain is
/// `ancestors`' walk (`library_core::shelf::tree`), not a hand-rolled one.
fn hangs_inside(shelves: &[Shelf], shelf_id: &str, folder_id: &str) -> bool {
    shelves_ops::ancestors(shelves, shelf_id)
        .iter()
        .any(|parent| parent.kind.folder_id() == Some(folder_id))
}

/// The rungs a folded member brings: every shelf the folded folder owns,
/// keyed the way the receiving tree keys its own — the rung the member's
/// directory names, and the ones below it.
fn member_rungs(shelves: &[Shelf], gone_id: &str, rel: &str) -> Vec<(String, String)> {
    shelves
        .iter()
        .filter(|s| s.kind.folder_id() == Some(gone_id))
        .filter_map(|s| {
            let ShelfKind::Folder { rel: own, .. } = &s.kind else {
                return None;
            };
            let own = own.as_deref().unwrap_or("");
            let key = if own.is_empty() {
                rel.to_string()
            } else {
                format!("{rel}/{own}")
            };
            Some((key, s.id.clone()))
        })
        .collect()
}

/// The rungs a run lands on when a member stands outside the tree it belongs
/// to, seeded into the run's map before the walk: the walk then places the
/// member's ground on the shelf that already stands for it instead of minting
/// a rung beside each one. `run_fold` asks the same question after the walk
/// and folds the member's row behind it.
///
/// Nothing is seeded for a copying row (its shelves are the library's own) or
/// for a row keeping its whole ground on one shelf: the member's rungs are
/// not rungs of that tree, so its books come onto the root rung — the
/// flattening the fold runs (`flatten_rungs`).
pub(super) fn seed_member_rungs(state: AppState, folder: &mut WatchedFolder, found: &[FoundFile]) {
    if !folder.mode().reads_in_place() {
        return;
    }
    let Some(member) = displaced_member(state, folder, found) else {
        return;
    };
    // A member's ground answers with the shape at it: where the tree keeps
    // that ground on one shelf there is no rung for the member's shelves to
    // come back onto.
    if !folder.cuts(&member.rel) {
        return;
    }
    let shelves = state.library.shelves.get_untracked();
    for (key, id) in member_rungs(&shelves, &member.folder_id, &member.rel) {
        folder.shelf_map.insert(key, id);
    }
}

/// Put a displaced member back on the rung its directory names and fold the
/// folder that was reading it into the tree that contains it: one ground, one
/// reader from here on. Three writes, in the order that keeps them honest;
/// the answer is the shelf the member sat on.
///
/// `run_root` is the root the calling run holds the claim for: a tree another
/// run is walking is the one fold to refuse, because that run's clone of the
/// ledger lands after this write and drops it. The calling run's own tree is
/// the row it is already writing, and the fold writes it last.
pub(crate) fn reclaim_rung(
    state: AppState,
    tree_id: &str,
    gone_id: &str,
    rel: &str,
    shelf_id: &str,
    run_root: Option<&str>,
) -> Option<String> {
    let now = now_ms();
    let mut minted: Vec<Shelf> = Vec::new();
    let (mut tree, gone) = state.library.folders.with_untracked(|folders| {
        let tree = folder_ops::find(folders, tree_id)?.clone();
        let gone = folder_ops::find(folders, gone_id)?.clone();
        Some((tree, gone))
    })?;
    // Read before anything is written: the answer is about the shelves
    // standing, not the ones this move mints.
    let rungs = state
        .library
        .shelves
        .with_untracked(|shelves| member_rungs(shelves, gone_id, rel));
    // A shelf that went while the note was up is an answer with nothing to
    // move; so is a tree a different run is walking, whose ledger clone lands
    // after this write.
    let foreign_walk = root_is_claimed(&tree.root) && run_root != Some(tree.root.as_str());
    if !rungs.iter().any(|(_, id)| id == shelf_id) || foreign_walk {
        return None;
    }
    let root = tree.root.clone();
    // Ground the shape keeps on one shelf has no rung for the member's
    // directory to become: its books come onto the rung the ground answers
    // for and its own shelves go, so an adoption cannot cut a nested rung
    // into an import that asked for none.
    let seat = if tree.cuts(rel) {
        let parent = chain_for(
            &mut tree,
            library_core::folder::parent_key(rel).unwrap_or(""),
            now,
            &root,
            &None,
            false,
            &mut minted,
        );
        for (key, id) in &rungs {
            tree.shelf_map.insert(key.clone(), id.clone());
        }
        state.library.shelves.update(|shelves| {
            page_into(shelves, minted);
            let nestable = shelves_ops::can_nest(shelves, shelf_id, &parent);
            for (key, id) in &rungs {
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
        shelf_id.to_string()
    } else {
        // A pointer to a shelf that went is no seat, and this tree's map is not the one the run
        // pruned: the mint is asked for the key the map has no standing rung under.
        let standing = state.library.shelves.with_untracked(|shelves| {
            tree.shelf_map
                .get("")
                .filter(|id| shelves_ops::find(shelves, id.as_str()).is_some())
                .cloned()
        });
        let seat = match standing {
            Some(seat) => seat,
            None => {
                tree.shelf_map.remove("");
                chain_for(&mut tree, "", now, &root, &None, false, &mut minted)
            }
        };
        page_shelves(state, minted);
        flatten_rungs(state, gone_id, &seat);
        seat
    };
    // The answer the folded row carried for its own root becomes the rung it becomes: the row that
    // answer was written on is the one this fold retires. A tree that cuts no rungs has one answer
    // for the whole of its ground — the reader's own about its root — and the adoption does not
    // second-guess it.
    if tree.cuts(rel) {
        tree.set_tracking(rel, gone.opts.watch);
    }
    tree.placed.extend(gone.placed.iter().copied());
    for stone in gone.ignored.iter() {
        if !tree.is_ignored(&stone.fp) {
            tree.ignored.push(stone.clone());
        }
    }
    tree.scanned_ms = tree.scanned_ms.max(gone.scanned_ms);

    state.library.folders.update(|folders| {
        folders.retain(|f| f.id != gone_id);
        match folders.iter().position(|f| f.id == tree_id) {
            Some(at) => folders[at] = tree,
            None => folders.push(tree),
        }
    });
    crate::storage::persist_library(state.library);
    Some(seat)
}

/// The half of [`import_folder`] that is the same whatever the folder sheet
/// decided, and the half its answers call directly.
pub(crate) fn proceed_folder(state: AppState, root: String, opts: FolderOpts, plan: RootPlan) {
    // A folder already being imported is an import already answering this ask: racing it would
    // clobber its ledger write. A RESCAN of the same tree is not refused — an ask outranks it.
    start_guarded(state, &root, move |state, task, root| {
        start_folder_run(state, root, opts, plan, task)
    });
}

/// One shape for the two starts an import has, because the two owe the same run and the same card.
fn start_folder_run(state: AppState, root: String, opts: FolderOpts, plan: RootPlan, task: String) {
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
/// outside the tree it belongs to. Either fold is told the root this run holds: a tree ANOTHER
/// run is walking is the one fold to refuse.
pub(super) fn run_fold(
    state: AppState,
    plan: &RootPlan,
    folder: &WatchedFolder,
    root_rung: Option<&str>,
    found: &[FoundFile],
) -> Option<(String, String)> {
    let seated = if let Some(Fold { tree_id: tree, rel }) = &plan.fold {
        let rung = root_rung?;
        reclaim_rung(state, tree, &folder.id, rel, rung, Some(&folder.root))?
    } else {
        let member = displaced_member(state, folder, found)?;
        reclaim_rung(
            state,
            &folder.id,
            &member.folder_id,
            &member.rel,
            &member.shelf_id,
            Some(&folder.root),
        )?
    };
    let name = state.library.shelf_name(&seated);
    Some((seated, name))
}
