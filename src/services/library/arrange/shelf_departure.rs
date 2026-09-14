//! The shelf's departure: a hand taking a read-at-place shelf off the seat its folder's tree
//! names. The copies are a cost, and a cost is a question — the ask, the sheet's three
//! answers, and the departure itself.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Origin, Row, book_rows, duplicate_title};
use library_core::folder::{self as folder_ops, WatchedFolder};
use library_core::shelf::{self as shelf, Shelf};

use crate::services::library::{folder_label, toast};
use crate::state::AppState;

use super::departure::depart;
use super::shelves::{nest_shelf, reorder_shelves_to_anchor};
use crate::services::library::reveal;

/// A value rather than a boolean, because "the drop was after" and "insert after the anchor" are one fact said at three call sites.
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

#[derive(Clone, PartialEq, Eq)]
pub struct DepartingShelf {
    pub id: String,
    pub name: String,
    pub folder_name: String,
    /// The books that become the library's own copies stand on the departing rungs.
    pub books: usize,
    /// The folder sheet's own convention: promised on the row before the click and counted again at it. The folder's own name stays free, because the next import of it re-mints the original tree wearing it.
    pub copy_name: String,
}

/// Its own ask rather than a variant of the name sheet's because nothing collides: a nesting
/// writes no membership, and the level the copies land on has nothing to say about them.
#[derive(Clone, PartialEq)]
pub struct ShelfDepartureAsk {
    /// A departing shelf filed inside one of these rides with it and asks nothing of its own.
    pub departing: Vec<DepartingShelf>,
    pub target: Option<String>,
    pub seam: Option<ShelfSeam>,
    /// Empty unless the drop landed inside a family: a list with nothing in it is a sheet with two answers.
    pub returns: Vec<ShelfReturn>,
}

#[derive(Clone, PartialEq)]
pub struct ShelfReturn {
    pub shelf_id: String,
    pub name: String,
    pub path: ReturnPath,
}

#[derive(Clone, PartialEq)]
pub enum ReturnPath {
    Reclaim {
        tree: String,
        gone: String,
        rel: String,
        family_name: String,
    },
    Reseat {
        seat: Option<String>,
        family_name: String,
    },
}

impl ShelfDepartureAsk {
    /// A `view!` body is a builder and not a place to compute, and the counts here walk every book
    /// the departing rungs hold. `None` when no departing shelf is there to ask about any more.
    pub(super) fn of(
        state: AppState,
        departing: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
    ) -> Option<Self> {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let books = state.library.books.get_untracked();
        let level = landing_level(&shelves, &target, seam.as_ref());
        let mut promised: std::collections::HashSet<String> =
            shelf::children_of(&shelves, level.as_deref())
                .into_iter()
                .map(|s| s.name.clone())
                .collect();
        let mut rows: Vec<DepartingShelf> = Vec::new();
        let mut returns: Vec<ShelfReturn> = Vec::new();
        for id in departing {
            let Some(one) = shelf::find(&shelves, &id) else {
                continue;
            };
            let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
                continue;
            };
            let folder = folders.iter().find(|f| &f.id == folder_id);
            let folder_name = folder
                .map(|f| folder_label(&f.root))
                .unwrap_or_else(|| one.name.clone());
            let (subtree, rungs) = departing_sets(&shelves, folder_id, &id);
            let count = folder.map_or(0, |f| {
                departing_book_ids(&books, &shelves, f, &rungs, &subtree).len()
            });
            // Offered only for a drop inside the mover's FAMILY: anywhere else the copy is the only honest answer, because there is no tree to put the shelf back into.
            let family_drop = folder.is_some_and(|f| {
                let ground =
                    folder_ops::dir_of_rung(&f.root, rel.as_deref().unwrap_or(""));
                target_is_family(&shelves, &folders, level.as_deref(), &ground)
            });
            if family_drop
                && let Some(path) = return_path(&shelves, &folders, &id)
            {
                returns.push(ShelfReturn {
                    shelf_id: id.clone(),
                    name: one.name.clone(),
                    path,
                });
            }
            let copy_name = duplicate_title(&one.name, &promised);
            promised.insert(copy_name.clone());
            rows.push(DepartingShelf {
                id,
                name: one.name.clone(),
                folder_name,
                books: count,
                copy_name,
            });
        }
        (!rows.is_empty()).then_some(Self {
            departing: rows,
            target,
            seam,
            returns,
        })
    }
}

/// The seam's anchor answers with the level that holds IT, a filing answers with its target, and the root is the level that is not a shelf.
fn landing_level(
    shelves: &[Shelf],
    target: &Option<String>,
    seam: Option<&ShelfSeam>,
) -> Option<String> {
    match seam {
        Some(seam) => shelf::find(shelves, &seam.anchor_id).and_then(|s| s.parent.clone()),
        None => target.clone(),
    }
}

/// A rung of an in-place tree whose root covers the mover's ground directory — the mover's
/// own tree included, whose rungs are its first family. A read-at-place shelf lives on the seat
/// its directory stands on, so a move inside the tree it belongs to can answer with the seat
/// instead of a copy.
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

/// Two shapes, and which one a shelf owes is a fact about where its folder stands. The
/// folder's ROOT shelf whose ground a family tree covers at a free rung goes home by the fold:
/// `reclaim_rung`, which hangs the shelf on the rung its directory names and folds the folder
/// that was reading it into the tree's ledger.
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
        let family_name = folders
            .iter()
            .find(|f| f.id == tree)
            .map(|f| folder_label(&f.root))
            .unwrap_or_default();
        return Some(ReturnPath::Reclaim {
            tree,
            gone: folder.id.clone(),
            rel: tree_rel,
            family_name,
        });
    }
    let seat = folder_ops::parent_key(&key)
        .and_then(|rung| folder.shelf_map.get(rung))
        .cloned();
    (one.parent != seat).then(|| ReturnPath::Reseat {
        seat,
        family_name: folder_label(&folder.root),
    })
}

/// The subtree is every shelf below the one the hand named — it rides with the copy the way a
/// directory's tree rides with the directory — and the rungs are the folder's OWN shelves
/// inside that subtree, which go free of the folder's map.
pub(super) fn departing_sets(
    shelves: &[Shelf],
    folder_id: &str,
    top_id: &str,
) -> (std::collections::HashSet<String>, std::collections::HashSet<String>) {
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

/// The linked books the folder placed whose own rung is one of the departing ones, and who are
/// members of the departing subtree. The two conditions each rule out a real shape: a book
/// whose rung stands OUTSIDE the subtree, and a book the folder placed but no longer holds.
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

/// One spelling for the three hand-moves, because a drag, a bulk filing and a sibling reorder
/// are one rule and one question. The rule itself is [`shelf::departing_moves`]' — pure, and
/// host-tested.
pub(super) fn screen_shelf_moves(
    state: AppState,
    ids: &[String],
    parent: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    if !tauri_bridge::has_tauri() {
        return (ids.to_vec(), Vec::new());
    }
    let shelves = state.library.shelves.get_untracked();
    state.library.folders.with_untracked(|folders| {
        shelf::departing_moves(&shelves, folders, ids, parent)
    })
}

/// One question per gesture and no queue: a drag is one act, and a second act while the sheet is up replaces it.
pub(super) fn raise_departure(
    state: AppState,
    departing: Vec<String>,
    target: Option<String>,
    seam: Option<ShelfSeam>,
) {
    let Some(ask) = ShelfDepartureAsk::of(state, departing, target, seam) else {
        return;
    };
    state.library.shelf_departure.raise(ask);
}

/// Nothing moves and nothing copies, and the clean half of the gesture keeps its landing.
pub fn cancel_departure(state: AppState) {
    state.library.shelf_departure.dismiss();
}

/// No copies: every mover that has a way home takes it, and a mover that has none stays where
/// the tree put it. The fold is the import's own `reclaim_rung`, and the reseat rides the very
/// `nest_shelf` the gesture did.
pub fn answer_departure_return(state: AppState) {
    let Some(ask) = state.library.shelf_departure.ask.get_untracked() else {
        return;
    };
    cancel_departure(state);
    let mut first: Option<String> = None;
    for ret in &ask.returns {
        let moved = match &ret.path {
            ReturnPath::Reclaim {
                tree,
                gone,
                rel,
                ..
            } => crate::services::library::import::reclaim_rung(state, tree, gone, rel, &ret.shelf_id),
            ReturnPath::Reseat { seat, .. } => {
                nest_shelf(state, &ret.shelf_id, seat.as_deref())
            }
        };
        if moved && first.is_none() {
            first = Some(ret.shelf_id.clone());
        }
    }
    if let Some(id) = first {
        reveal::reveal_shelf(state, &id);
    }
}

/// The copies run in a spawned task — a shelf of fifty books is fifty files through the store — and the sheet is off the screen at once.
pub fn confirm_departure(state: AppState) {
    let Some(ask) = state.library.shelf_departure.ask.get_untracked() else {
        return;
    };
    cancel_departure(state);
    if ask.departing.is_empty() || !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        depart_shelves(state, ask).await;
    });
}

/// The book departure's order read one level up: the copies are made and the rungs are converted BEFORE any shelf write happens.
async fn depart_shelves(state: AppState, ask: ShelfDepartureAsk) {
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
        landing_level(&shelves, &ask.target, ask.seam.as_ref())
    };
    let mut departures: Vec<Departure> = Vec::new();
    {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let books = state.library.books.get_untracked();
        for row in &ask.departing {
            let Some(one) = shelf::find(&shelves, &row.id) else {
                continue;
            };
            let shelf::ShelfKind::Folder { folder_id, rel } = &one.kind else {
                continue;
            };
            let Some(folder) = folders.iter().find(|f| &f.id == folder_id) else {
                continue;
            };
            // The rule is asked again, because the sheet was up while the library went on living: a shelf that no longer owes a departure is skipped silently.
            if !shelf::departs_on_move(&shelves, &folders, &row.id, level.as_deref()) {
                continue;
            }
            if let Some(parent) = &level
                && !shelf::can_nest(&shelves, &row.id, parent)
            {
                continue;
            }
            let (subtree, rungs) = departing_sets(&shelves, folder_id, &row.id);
            let book_ids = departing_book_ids(&books, &shelves, folder, &rungs, &subtree);
            departures.push(Departure {
                id: row.id.clone(),
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

    // Bytes into the store, a moved-out log into every folder that placed the book, the name
    // pinned into the title, and one cover ask for the batch. What is left here is the shelf's own
    // bookkeeping.
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

    // The rungs leave the tree with the copy they paid for, the other folders' shelves that rode along take the hand's mark, and the folder lets the departed zone go.
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
                let copy_name = duplicate_title(&dep.name, &promised);
                promised.insert(copy_name.clone());
                one.name = copy_name;
            }
        }
    });
    state.library.folders.update(|folders| {
        for dep in &going {
            let Some(folder) = folder_ops::find_mut(folders, &dep.folder_id) else {
                continue;
            };
            folder.shelf_map.retain(|key, shelf_id| {
                !folder_ops::key_in_zone(key, &dep.rel)
                    && !dep.rungs.contains(shelf_id)
            });
        }
    });
    crate::storage::persist_library(state.library);

    if let Some(seam) = &ask.seam {
        reorder_shelves_to_anchor(state, &landed, &seam.anchor_id, seam.side);
    } else {
        for id in &landed {
            nest_shelf(state, id, level.as_deref());
        }
    }
}
