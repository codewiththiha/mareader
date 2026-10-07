//! The shelf's departure: a folder shelf taken off its seat by hand.

use leptos::prelude::*;

use library_core::book::{Origin, Row, book_rows};
use library_core::folder::{self as folder_ops, WatchedFolder};
use library_core::shelf::{self as shelf, Shelf};

use crate::services::toast;

use super::asking::free_name;
use super::departure::depart;
use super::shelves::{nest_shelf, reorder_shelves_to_anchor};
use crate::services::reveal;

/// One fact said at three call sites: "the drop was after".
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeamSide {
    Before,
    After,
}

/// A filing has no seam and appends.
#[derive(Clone, PartialEq, Eq)]
pub struct ShelfSeam {
    pub anchor_id: String,
    pub side: SeamSide,
}

/// Where the gesture's shelves were going: a named shelf, or the root.
#[derive(Clone, PartialEq)]
pub(super) enum ReturnPath {
    /// A displaced root shelf with a family tree over its ground folds back.
    Reclaim {
        tree: String,
        gone: String,
        rel: String,
    },
    /// Any other off-seat rung reseats under its directory's shelf.
    Reseat { seat: Option<String> },
}

/// The seam answers with the level that holds the anchor; a filing, its target.
pub(super) fn landing_level(
    shelves: &[Shelf],
    target: &Option<String>,
    seam: Option<&ShelfSeam>,
) -> Option<String> {
    match seam {
        Some(seam) => shelf::find(shelves, &seam.anchor_id).and_then(|s| s.parent.clone()),
        None => target.clone(),
    }
}

/// A rung of an in-place tree over the mover's ground: the seat, not a copy.
pub(super) fn target_is_family(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    target: Option<&str>,
    ground: &str,
) -> bool {
    let Some(target) = target else {
        return false;
    };
    let Some(one) = shelf::find(shelves, target) else {
        return false;
    };
    let Some(folder_id) = one.kind.folder_id() else {
        return false;
    };
    folders.iter().any(|f| {
        f.id == folder_id
            && f.mode().reads_in_place()
            && folder_ops::rel_under(ground, &f.root).is_some()
    })
}

/// Which shape a shelf owes is a fact about where its folder stands.
pub(super) fn return_path(
    shelves: &[Shelf],
    folders: &[WatchedFolder],
    shelf_id: &str,
) -> Option<ReturnPath> {
    let one = shelf::find(shelves, shelf_id)?;
    let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
        return None;
    };
    let folder = folders
        .iter()
        .find(|f| &f.id == folder_id && f.mode().reads_in_place())?;
    let key = rel.clone().unwrap_or_default();
    if key.is_empty()
        && let Some((tree, tree_rel)) = shelf::family_for(folders, shelves, &folder.root)
    {
        return Some(ReturnPath::Reclaim {
            tree,
            gone: folder.id.clone(),
            rel: tree_rel,
        });
    }
    let seat = folder_ops::parent_key(&key)
        .and_then(|rung| folder.shelf_map.get(rung))
        .cloned();
    (one.parent != seat).then_some(ReturnPath::Reseat { seat })
}

/// The shelf below the hand and its rungs; the rungs go free of the map.
pub(super) fn departing_sets(
    shelves: &[Shelf],
    folder_id: &str,
    top_id: &str,
) -> (
    std::collections::HashSet<String>,
    std::collections::HashSet<String>,
) {
    let root = [top_id.to_string()];
    let subtree: std::collections::HashSet<String> = std::iter::once(top_id.to_string())
        .chain(shelf::subtree_ids(shelves, &root))
        .collect();
    let rungs: std::collections::HashSet<String> = subtree
        .iter()
        .filter(|id| {
            shelf::find(shelves, id).is_some_and(|s| s.kind.folder_id() == Some(folder_id))
        })
        .cloned()
        .collect();
    (subtree, rungs)
}

/// The folder's linked books inside the departing subtree, by their rungs.
pub(super) fn departing_book_ids(
    books: &[Row],
    shelves: &[Shelf],
    folder: &WatchedFolder,
    rungs: &std::collections::HashSet<String>,
    subtree: &std::collections::HashSet<String>,
) -> Vec<String> {
    book_rows(books)
        .filter(|b| matches!(b.origin, Origin::Linked { .. }) && folder.placed.contains(&b.fp))
        .filter(|b| {
            folder
                .rungs_for(b.path())
                .0
                .is_some_and(|rung| rungs.contains(rung))
        })
        .filter(|b| {
            shelf::containing(shelves, &b.id)
                .iter()
                .any(|s| subtree.contains(&s.id))
        })
        .map(|b| b.id.clone())
        .collect()
}

/// One spelling for the three hand-moves; [`shelf::departing_moves`] rules.
pub(super) fn screen_shelf_moves(
    state: crate::context::LibraryContext,
    ids: &[String],
    parent: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    if !tauri_bridge::has_tauri() {
        return (ids.to_vec(), Vec::new());
    }
    let shelves = state.library.shelves.get_untracked();
    state
        .library
        .folders
        .with_untracked(|folders| shelf::departing_moves(&shelves, folders, ids, parent))
}

/// No copies: every mover with a way home takes it.
pub(super) fn take_them_home(
    state: crate::context::LibraryContext,
    returns: &[(String, ReturnPath)],
) {
    let mut first: Option<String> = None;
    for (shelf_id, path) in returns {
        let moved = match path {
            ReturnPath::Reclaim { tree, gone, rel } => {
                crate::services::import::reclaim_rung(state, tree, gone, rel, shelf_id, None)
            }
            ReturnPath::Reseat { seat } => {
                nest_shelf(state, shelf_id, seat.as_deref()).then(|| shelf_id.clone())
            }
        };
        if let Some(seated) = moved {
            first.get_or_insert(seated);
        }
    }
    if let Some(id) = first {
        reveal::reveal_shelf(state, &id);
    }
}

/// The landing the reader bought: copies first, before any shelf write.
pub(super) async fn take_shelves_out(
    state: crate::context::LibraryContext,
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) {
    struct Departure {
        id: String,
        name: String,
        folder_id: String,
        rel: String,
        rungs: std::collections::HashSet<String>,
        subtree: std::collections::HashSet<String>,
        books: Vec<String>,
    }

    let level: Option<String> = {
        let shelves = state.library.shelves.get_untracked();
        landing_level(&shelves, &target, seam.as_ref())
    };
    let mut departures: Vec<Departure> = Vec::new();
    {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let books = state.library.books.get_untracked();
        for id in &departing {
            let Some(one) = shelf::find(&shelves, id) else {
                continue;
            };
            let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
                continue;
            };
            let Some(folder) = folders.iter().find(|f| &f.id == folder_id) else {
                continue;
            };
            // Asked again: the sheet was up while the library lived.
            if !shelf::departs_on_move(&shelves, &folders, id, level.as_deref()) {
                continue;
            }
            if let Some(parent) = &level
                && !shelf::can_nest(&shelves, id, parent)
            {
                continue;
            }
            let (subtree, rungs) = departing_sets(&shelves, folder_id, id);
            let book_ids = departing_book_ids(&books, &shelves, folder, &rungs, &subtree);
            departures.push(Departure {
                id: id.clone(),
                name: one.name.clone(),
                folder_id: folder_id.clone(),
                rel: rel.clone().unwrap_or_default(),
                rungs,
                subtree,
                books: book_ids,
            });
        }
    }
    if departures.is_empty() {
        return;
    }

    // Bytes into the store, a log per folder, a pinned name, one cover ask.
    let mut landed: Vec<String> = Vec::new();
    for dep in &departures {
        let copied = depart(state, &dep.books).await;
        if dep.books.is_empty() || !copied.is_empty() {
            landed.push(dep.id.clone());
        } else {
            toast(
                state,
                format!(
                    "“{}” stayed where it was — the library could not copy its books.",
                    dep.name
                ),
            );
        }
    }
    if landed.is_empty() {
        return;
    }
    let going: Vec<&Departure> = departures
        .iter()
        .filter(|dep| landed.iter().any(|id| id == &dep.id))
        .collect();

    // The rungs leave with the copy they paid for; the rest take the mark.
    let mut promised: std::collections::HashSet<String> =
        state.library.shelves.with_untracked(|shelves| {
            shelf::children_of(shelves, level.as_deref())
                .into_iter()
                .map(|s| s.name.clone())
                .collect()
        });
    state.library.shelves.update(|shelves| {
        for dep in &going {
            for rung in &dep.rungs {
                if let Some(one) = shelf::find_mut(shelves, rung) {
                    one.kind = shelf::ShelfKind::Departed;
                    one.manual_parent = false;
                }
            }
            for id in &dep.subtree {
                if dep.rungs.contains(id) {
                    continue;
                }
                if let Some(one) = shelf::find_mut(shelves, id)
                    && one.is_folder()
                {
                    one.manual_parent = true;
                }
            }
            if let Some(one) = shelf::find_mut(shelves, &dep.id) {
                one.name = free_name(&dep.name, &mut promised);
            }
        }
    });
    state.library.folders.update(|folders| {
        for dep in &going {
            let Some(folder) = folder_ops::find_mut(folders, &dep.folder_id) else {
                continue;
            };
            folder.shelf_map.retain(|key, shelf_id| {
                !folder_ops::key_in_zone(key, &dep.rel) && !dep.rungs.contains(shelf_id)
            });
        }
    });
    crate::services::persist_library(state.library);

    if let Some(seam) = &seam {
        reorder_shelves_to_anchor(state, &landed, &seam.anchor_id, seam.side);
    } else {
        for id in &landed {
            nest_shelf(state, id, level.as_deref());
        }
    }
}
