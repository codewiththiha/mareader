//! The shelf's departure: a hand taking a read-at-place shelf off the seat
//! its folder's tree names. The copies are a cost, and a cost is a question —
//! the ask, the sheet's three answers (pay it, leave the shelf where the tree
//! put it, or take the way home when the drop landed inside the family) and
//! the departure itself.

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

/// Which side of an anchor a sibling seam lands on. A value rather than a
/// boolean, because "the drop was after" and "insert after the anchor" are
/// one fact said at three call sites, and a bare `true` at one of them is a
/// fact nobody can read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SeamSide {
    Before,
    After,
}

/// The sibling seam a shelf-row's edge named: the anchor the copies land
/// beside, and which side of it. A filing has no seam and appends.
#[derive(Clone, PartialEq, Eq)]
pub struct ShelfSeam {
    pub anchor_id: String,
    pub side: SeamSide,
}

/// One read-at-place shelf a gesture is about to turn into the library's own
/// copy: the facts the departure sheet speaks.
#[derive(Clone, PartialEq, Eq)]
pub struct DepartingShelf {
    pub id: String,
    /// The name the shelf wore when the question was asked.
    pub name: String,
    /// What the folder that reads it in place is called.
    pub folder_name: String,
    /// How many read-at-place books standing on the departing rungs become the
    /// library's own copies with it.
    pub books: usize,
    /// The name promised to the copy at the level it lands on — the counter a
    /// second instance wears, promised on the row before the click and counted
    /// again at it, which is the folder sheet's own convention. The folder's
    /// own name stays free, because the next import of it re-mints the
    /// original tree wearing it.
    pub copy_name: String,
}

/// The departure question on screen: the shelves about to become the
/// library's own copies, where the gesture meant to land them, and the way
/// home each of them has when the drop was inside its family.
///
/// Its own ask rather than a variant of the name sheet's because nothing
/// collides: a nesting writes no membership, and the level the copies land on
/// has nothing to say about them. The question is what the move COSTS — the
/// copies a read-at-place shelf owes when it leaves its folder's ground — and
/// its answers are to pay it, to leave the shelf where the tree put it, or,
/// when the drop landed inside the shelf's FAMILY, to put the shelf back
/// where its folder names instead: a read-at-place shelf lives on the seat
/// its directory stands on, so a move inside the tree it belongs to never
/// has to cost a copy.
#[derive(Clone, PartialEq)]
pub struct ShelfDepartureAsk {
    /// The shelves that owe the ask, in the order the gesture named them. A
    /// departing shelf filed inside one of these rides with it and asks
    /// nothing of its own.
    pub departing: Vec<DepartingShelf>,
    /// The parent the copies land inside. `None` is the library's root, which
    /// is a level rather than a shelf.
    pub target: Option<String>,
    /// The sibling seam, when the drop named a position rather than a mouth.
    pub seam: Option<ShelfSeam>,
    /// The movers that have a way home, and the way each one takes. Empty
    /// unless the drop landed inside a family: the sheet's third answer is
    /// this list, and a list with nothing in it is a sheet with two answers.
    pub returns: Vec<ShelfReturn>,
}

/// One departing shelf's way home, when it has one: the shelf, and the path
/// that puts it back where its folder names.
#[derive(Clone, PartialEq)]
pub struct ShelfReturn {
    pub shelf_id: String,
    /// The name the shelf wore when the question was asked.
    pub name: String,
    pub path: ReturnPath,
}

/// The way home, of which there are two shapes — and which one a shelf owes
/// is a fact about where its folder stands, not about the drop.
#[derive(Clone, PartialEq)]
pub enum ReturnPath {
    /// The shelf is its folder's root and a FAMILY tree covers its ground at
    /// a rung no shelf wears: the answer folds the folder back into the tree
    /// — the import's own `reclaim_rung`, the ledger folded in, the shelf on
    /// the rung its directory names.
    Reclaim {
        tree: String,
        gone: String,
        rel: String,
        /// What the family's tree is called, which is what the sheet says.
        family_name: String,
    },
    /// The shelf is off the seat its own folder's ledger names: the answer
    /// re-seats it there, and the hand's mark comes off on the way — the
    /// disk owns the place again.
    Reseat {
        seat: Option<String>,
        /// What the folder is called, which is what the sheet says.
        family_name: String,
    },
}

impl ShelfDepartureAsk {
    /// The question, read once out of the library: the sheet's own rule, a
    /// `view!` body is a builder and not a place to compute, and the counts
    /// here walk every book the departing rungs hold.
    ///
    /// `None` when no departing shelf is there to ask about any more: a
    /// gesture with nothing left to copy owes the reader no sheet.
    ///
    /// The promised names are counted against the level SEQUENTIALLY: two
    /// rungs landing beside one another cannot both wear `Fiction_1`, and the
    /// second promise is counted as if the first had landed, which is what the
    /// click's recount then does for real.
    pub(super) fn of(
        state: AppState,
        departing: Vec<String>,
        target: Option<String>,
        seam: Option<ShelfSeam>,
    ) -> Option<Self> {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        let books = state.library.books.get_untracked();
        // The level the copies land on: a seam's anchor names it, a filing's
        // target does, and the open page names neither.
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
            // The way home is offered only for a drop inside the mover's
            // FAMILY — a rung of an in-place tree that covers the ground the
            // mover stands on. Anywhere else the copy is the only honest
            // answer: there is no tree to put the shelf back into.
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

/// The level a landing puts its shelves on: the seam's anchor answers with the
/// level that holds IT, a filing answers with its target, and the root is the
/// level that is not a shelf.
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

/// Whether the drop's target is inside the mover's FAMILY: a rung of an
/// in-place tree whose root covers the mover's ground directory — the mover's
/// own tree included, whose rungs are its first family.
///
/// The family drop is the one that offers the way home: a read-at-place shelf
/// lives on the seat its directory stands on, so a move inside the tree it
/// belongs to can always answer with the seat instead of a copy. The root
/// level is nobody's family — "All" is a level and not a shelf — and a
/// reader's own shelf is a place, not a tree.
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

/// The mover's way home, when it has one.
///
/// Two shapes, and which one a shelf owes is a fact about where its folder
/// stands. The folder's ROOT shelf whose ground a family tree covers at a
/// free rung goes home by the fold: the import's own `reclaim_rung`, which
/// hangs the shelf on the rung its directory names, rewrites its kind to the
/// tree's, and folds the folder that was reading it into the tree's ledger.
/// Any shelf OFF the seat its own ledger names goes home by the reseat: the
/// reparent that seats it back, which takes the hand's mark off on the way.
/// A shelf already on its seat has no way home — it is home — and the move
/// that named it has the copy's answer or the cancel's.
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

/// The set a departing shelf takes with it, and the rungs inside it that the
/// departure converts: the subtree is every shelf below the one the hand
/// named — it rides with the copy the way a directory's tree rides with the
/// directory — and the rungs are the folder's OWN shelves inside that subtree,
/// which turn into the reader's own and go free of the folder's map.
///
/// A shelf of ANOTHER folder inside the subtree is not a rung of this
/// departure: it rides, keeps its disk knowledge, and takes the hand's mark so
/// its own folder's re-hang leaves it where the copy put it.
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

/// The read-at-place books standing on the departing rungs: the linked books
/// the folder placed whose own rung is one of the departing ones, and who are
/// members of the departing subtree — the shelf departure's conversion set,
/// the books that become the library's own copies with their shelf.
///
/// The two conditions each rule out a real shape. A book whose rung stands
/// OUTSIDE the subtree — a second membership of the departing shelf, an
/// "also show it here" — has not left its ground: its rung still stands, the
/// ledger still answers for it, and it keeps its link and both memberships,
/// exactly as a book on a rung that did not move does. And a book no shelf of
/// the subtree holds is not riding it, whatever the folder placed: a departure
/// that copied one would be a copy of a book that never moved.
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

/// Split a batch of requested shelf moves into the half that lands as it is
/// and the half that owes the departure's ask — the screen every hand-move
/// rides, one spelling for the three, because a drag, a bulk filing and a
/// sibling reorder are one rule and one question. The rule itself is
/// [`shelf::departing_moves`]' — pure, and host-tested.
///
/// The departing half does not move yet: [`raise_departure`] takes it to the
/// sheet, and [`confirm_departure`] runs the move again over the copies, the
/// book departure's own shape. The clean half lands now, and a cancel leaves
/// it landed — the conflict sheet's cancel semantics: the placements already
/// made keep their answers, and the ones the question was about simply do not
/// happen.
///
/// Without a shell there is no store to copy into and no departure anywhere,
/// which is the book gate's own answer for the same empty room: a browser
/// moves shelves the way it always has.
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

/// Put the departure question on screen. One question per gesture and no
/// queue: a drag is one act, and a second act while the sheet is up replaces
/// it — the first gesture's departure simply never landed, which is what its
/// cancel would have meant.
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

/// Walk away from the departure question: nothing moves, nothing copies, and
/// the clean half of the gesture — the books and the reader's own shelves that
/// landed before the sheet rose — keeps its landing.
pub fn cancel_departure(state: AppState) {
    state.library.shelf_departure.dismiss();
}

/// The sheet's family answer: no copies — every mover that has a way home
/// takes it, and a mover that has none stays where the tree put it.
///
/// The fold is the import's own `reclaim_rung` — the same arithmetic a family
/// import runs, the same ledger folded — and the reseat rides the very
/// `nest_shelf` the gesture did, whose screen sees a move onto the seat and
/// waves it through: the reparent seats the shelf and takes the hand's mark
/// off, and the disk owns the place again. The light lands on the first shelf
/// that moved, so the answer ends on the shelf in the place the sentence
/// promised rather than on a modal claiming it worked.
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

/// The sheet's answer that pays: the copies run in a spawned task — a shelf
/// of fifty books is fifty files through the store — and the sheet is off the
/// screen at once, the import's own shape: the dock and the toasts own the
/// feedback from here on.
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

/// The departure itself: the shelves the sheet asked about become the
/// library's own copies, land where the gesture meant, and the folders that
/// read them let the departed zone go.
///
/// The order is the whole of the rule, and it is the book departure's order
/// read one level up. The copies are made and the rungs are converted BEFORE
/// any shelf write happens, so the re-dispatched landing — and the screen
/// inside it — sees the shelves as what they are about to be: the copies are
/// the reader's own now, and the gate waves them through as the membership
/// edit a virtual shelf's move always was. A copy that fails costs that book
/// its bytes and nothing else: it rides along linked, still reading its file
/// at its place, and the toast says so. A shelf whose copies ALL failed
/// departs not at all and stays where the folder's tree put it, because a move
/// that cannot keep the promise the sheet made is a move that did not happen.
async fn depart_shelves(state: AppState, ask: ShelfDepartureAsk) {
    /// One shelf's departure, read whole before anything is written: the books'
    /// rung answers come out of the folder's map, and the structure pass below
    /// is what clears it.
    struct Departure {
        id: String,
        name: String,
        folder_id: String,
        rel: String,
        rungs: std::collections::HashSet<String>,
        subtree: std::collections::HashSet<String>,
        books: Vec<String>,
    }

    // The level the copies land on, re-read at the click rather than trusted
    // from the raise: a seam's anchor may have moved while the sheet was up,
    // and both the gate below and the promised names count against the level
    // as it stands.
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
            // The rule is asked again, because the sheet was up while the
            // library went on living: a rescan can have seated the shelf back
            // on its seat, a removal can have taken it, and a target that
            // would close a loop is a refusal the drop answered "no" by doing
            // nothing. A shelf that no longer owes a departure is skipped
            // silently — the stale half of a gesture is not news.
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

    // The copies, through the departure's ONE primitive: bytes into the store, a
    // moved-out log into every folder that placed the book, the name pinned into
    // the title, and one cover ask for the batch. What is left here is the
    // shelf's own bookkeeping — which shelves landed, and the sentence a shelf
    // owes when not one of its books could be copied.
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

    // The structure pass: the rungs become the reader's own shelves, the other
    // folders' shelves that rode along take the hand's mark, the copies take
    // the level's next free names, and the folder lets the departed zone go —
    // every rung key inside it, and every entry pointing at a shelf the
    // conversion took, so the next walk of the folder mints the original tree
    // again on the seats the disk names.
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
                    one.kind = shelf::ShelfKind::Virtual;
                    one.manual_parent = false;
                }
            }
            for id in &dep.subtree {
                if dep.rungs.contains(id) {
                    continue;
                }
                // A shelf of another folder that the subtree carried here: the
                // hand put it where it is, and its own folder's re-hang passes
                // it by — the mark's whole job.
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
    // No cover ask here: `depart` already queued one for the copies, and the
    // structure pass above renames shelves rather than landing rows, so there is
    // nothing new for the queue to want.

    // The landing, re-dispatched through the very functions the gesture rode:
    // the copies are virtual shelves now, so the screen inside sees nothing to
    // ask and the move lands as the membership edit it became.
    if let Some(seam) = &ask.seam {
        reorder_shelves_to_anchor(state, &landed, &seam.anchor_id, seam.side);
    } else {
        for id in &landed {
            nest_shelf(state, id, level.as_deref());
        }
    }
}
