//! The read-at-place gate in front of a folder import.

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
use crate::services::conflict;
use crate::services::folder_label;
use runtime_contract::time::now_ms;

/// The shelf a run continues onto when the gate already knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Continuation {
    pub shelf_id: String,
    pub name: String,
}

/// A pick folded into its tree: the ledger row and the rung.
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
    /// The rung the pick answered for; `""` is the tree's own ground.
    pub rung: String,
}

impl RootPlan {
    /// The pick's rung: the fold's, else the ground's own.
    pub(super) fn answered_rung(&self) -> String {
        match &self.fold {
            Some(Fold { rel, .. }) => rel.clone(),
            None => self.rung.clone(),
        }
    }
}

/// A held name is a question, and a read-at-place pick asks the family first.
pub fn import_folder(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    watch: Option<GroundWatch>,
) {
    if let Some(watch) = watch {
        set_rung_tracking(state, &watch);
    }
    if opts.mode().reads_in_place() {
        if let Some(covered) = covered_shelf(state, &root) {
            // The walk runs on the tree's ledger; the continuation names it.
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
        // A rung deleted or gone: the pick wants it back in its tree.
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

/// A named value, not a tuple: the branch lights `shelf_id`.
pub(super) struct Covered {
    pub shelf_id: String,
    pub shelf_name: String,
    /// The covering tree's ledger row; tracking is written at this id.
    pub tree_id: String,
    pub tree_root: String,
    /// `""` when the ground is the tree's root.
    pub rel: String,
}

/// What the library already reads answers the import sheet with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroundWatch {
    pub tree_id: String,
    pub rung: String,
    pub on: bool,
    pub opts: FolderOpts,
}

/// The rung a pick answers for and its tracking; `None` if no row does.
pub fn ground_tracking(state: crate::context::LibraryContext, root: &str) -> Option<GroundWatch> {
    // One snapshot answers both, so no rung is read off a stale list.
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    let (tree_id, rung) = match covered_of(&folders, &shelves, root) {
        Some(covered) => (covered.tree_id, covered.rel),
        None => shelves_ops::family_for(&folders, &shelves, root)?,
    };
    let row = folder_ops::find(&folders, &tree_id)?;
    // The sheet opens on the row's answers except the shape.
    let mut opts = row.opts.clone();
    opts.groups = row.shape_at(&rung);
    Some(GroundWatch {
        on: row.tracks_rung(&rung),
        opts,
        tree_id,
        rung,
    })
}

/// The write the sheet owes, at the picked rung; unchanged is `false`.
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

/// The walk a new decision owes is the import's own.
fn set_rung_tracking(state: crate::context::LibraryContext, watch: &GroundWatch) {
    let mut changed = false;
    state.library.folders.update(|folders| {
        changed = write_rung_tracking(folders, watch);
    });
    if changed {
        crate::services::persist_library(state.library);
    }
}

pub(super) fn covered_shelf(state: crate::context::LibraryContext, root: &str) -> Option<Covered> {
    let folders = state.library.folders.get_untracked();
    let shelves = state.library.shelves.get_untracked();
    covered_of(&folders, &shelves, root)
}

/// A folder's own tree answers for it before one it stands inside.
fn covered_of(folders: &[WatchedFolder], shelves: &[Shelf], root: &str) -> Option<Covered> {
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

/// The member standing outside the tree, made on its own before.
pub(super) fn displaced_member(
    state: crate::context::LibraryContext,
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
        // By its map first, then its kind.
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

/// Bounded by the list, so a cycle answers no rather than spinning.
fn hangs_inside(shelves: &[Shelf], shelf_id: &str, folder_id: &str) -> bool {
    shelves_ops::ancestors(shelves, shelf_id)
        .iter()
        .any(|parent| parent.kind.folder_id() == Some(folder_id))
}

/// The rungs a folded member brings, keyed the tree's own way.
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

/// The rungs a run seeds so the walk reuses them rather than minting.
pub(super) fn seed_member_rungs(
    state: crate::context::LibraryContext,
    folder: &mut WatchedFolder,
    found: &[FoundFile],
) {
    if !folder.mode().reads_in_place() {
        return;
    }
    let Some(member) = displaced_member(state, folder, found) else {
        return;
    };
    // Where the ground is one shelf, the member has no rung.
    if !folder.cuts(&member.rel) {
        return;
    }
    let shelves = state.library.shelves.get_untracked();
    for (key, id) in member_rungs(&shelves, &member.folder_id, &member.rel) {
        folder.shelf_map.insert(key, id);
    }
}

/// Put a displaced member back and fold its reader into the tree.
pub(crate) fn reclaim_rung(
    state: crate::context::LibraryContext,
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
    // Read before the write: the answer is about standing shelves.
    let rungs = state
        .library
        .shelves
        .with_untracked(|shelves| member_rungs(shelves, gone_id, rel));
    // A shelf that went, or a tree another run is walking, is refused.
    let foreign_walk = root_is_claimed(&tree.root) && run_root != Some(tree.root.as_str());
    if !rungs.iter().any(|(_, id)| id == shelf_id) || foreign_walk {
        return None;
    }
    let root = tree.root.clone();
    // One-shelf ground has no rung for the member's directory.
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
        // A pointer to a shelf that went is no seat.
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
    // The folded row's answer for its root becomes the rung it becomes.
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
    crate::services::persist_library(state.library);
    Some(seat)
}

/// The half of [`import_folder`] its answers call directly.
pub(crate) fn proceed_folder(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
) {
    // A folder already importing is answering this ask; a rescan is not
    // refused.
    start_guarded(state, &root, move |state, task, root| {
        start_folder_run(state, root, opts, plan, task)
    });
}

/// One shape for the two starts an import has.
fn start_folder_run(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    plan: RootPlan,
    task: String,
) {
    let Some(claim) = claim_root(&root, Asked::Explicitly) else {
        // The card is already up, so it is closed rather than counting.
        finish_task(state, &task, 0, 0);
        return;
    };
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::Explicitly, plan).await;
    });
}

/// The plan's own fold first, then a standing member's; foreign walks refuse.
pub(super) fn run_fold(
    state: crate::context::LibraryContext,
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
