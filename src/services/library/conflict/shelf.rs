//! The folder's own question, asked BEFORE the walk: the level already holds
//! the NAME the arriving folder would wear. Which answers the sheet offers is
//! the arrival's mode — a stored arrival gets the level's three, a
//! read-at-place arrival gets the pointer and the merge.

use leptos::prelude::*;

use library_core::conflict::{next_shelf_name, Arrival, Placement, PlacementAsk};
use library_core::folder::FolderOpts;
use library_core::shelf;

use crate::services::library::reveal;
use crate::services::library::toast;
use crate::state::AppState;

/// The folder question on screen: the name arriving, and the shelf already
/// here wearing it.
///
/// A separate ask rather than a variant of [`ConflictAsk`] because its answers
/// are about a whole import run rather than about one placement: the ones that
/// import start the run again with a plan, and the run's root and options are
/// facts no book collision has. The [`Arrival`] it builds is the name-only one
/// ([`Arrival::folder`]) — nothing has been measured when a NAME is the question
/// — and the answers themselves are the unified placements, applied by
/// `super::apply_placement` on a shelf scope.
#[derive(Clone, PartialEq)]
pub struct ShelfConflictAsk {
    /// What the arriving folder would be called: the last segment of its
    /// path, the name the reader picked it by.
    pub incoming_name: String,
    /// The shelf already at the level, which *make link* points at and
    /// *merge* files into.
    pub existing_id: String,
    pub existing_name: String,
    /// The import the question interrupted, kept whole: an answer runs it.
    pub root: String,
    pub opts: FolderOpts,
    /// Whether the shelf that holds the name is the arriving folder's OWN —
    /// the one its previous run minted, which its `shelf_map` still names. A
    /// re-import of one folder is a continuation rather than an arrival, and
    /// the sheet words it as one. Own or not, the arrival's MODE decides the
    /// answers: a stored arrival gets the level's own three — go and look at
    /// the shelf that is here, replace it, or a shelf of the next free name
    /// — and a read-at-place arrival of a DIFFERENT folder's name keeps the
    /// pointer and the merge, its *as new* withheld as the second instance
    /// the family gate exists to prevent.
    pub own: bool,
}

/// Which answers the folder's sheet offers, in the unified vocabulary.
///
/// The arrival's MODE decides, and the two sets are [`Placement`]'s own. A
/// READ-AT-PLACE arrival gets the pointer and the merge (*link*, *merge*), its
/// *keep both* withheld as the second instance of one ground the family gate
/// exists to prevent, and *replace* with it — neither side of a read-at-place
/// collision is the level's to empty. Everything else gets the level's own three
/// (*open*, *replace*, *keep both*): a STORED arrival is copies the library owns
/// and unrelated to any tree, and a read-at-place re-pick of the folder's OWN
/// shelf is a continuation of the tree the reader already has rather than an
/// arrival from outside it.
///
/// One function rather than a branch in the sheet and a second in the answer, so
/// the two cannot drift about which buttons a given arrival gets — which is what
/// they did when each spelled the condition out.
pub fn offers(ask: &ShelfConflictAsk) -> &'static [Placement] {
    if ask.opts.mode().reads_in_place() {
        Placement::SHELF_READ_IN_PLACE
    } else {
        Placement::SHELF_STORED
    }
}

/// Put the folder question on screen. One question, no queue: a folder import
/// is one run, and the run does not start until it is answered.
pub fn raise_shelf(state: AppState, ask: ShelfConflictAsk) {
    state.library.shelf_conflict.raise(ask);
}

/// One of the folder sheet's buttons, in the unified vocabulary.
///
/// The sheet renders [`offers`] and hands back a [`Placement`]; the write is
/// `super::apply_placement` on a shelf scope, which is the same dispatch a book
/// collision reaches — the branch that makes a shelf answer different from a row
/// answer is inside it, not here.
///
/// *Open* is the one answer that is not a placement of the arrival: it imports
/// nothing and lights the shelf that holds the name, wherever it hangs.
pub fn answer_shelf(state: AppState, answer: Placement) {
    let Some(ask) = state.library.shelf_conflict.ask.with_untracked(|a| a.clone()) else {
        return;
    };
    if !offers(&ask).contains(&answer) {
        return;
    }
    let placement = PlacementAsk::shelf(
        // A folder arrival has no measured file and no row of its own yet — the
        // NAME is the question — so the arrival carries the name the shelf would
        // wear and the level it would wear it on.
        Arrival::folder(ask.incoming_name.clone(), shelf::ALL_SHELF),
        ask.existing_id.clone(),
        ask.existing_name.clone(),
        offers(&ask),
    );
    if answer == Placement::Open {
        // The one answer that places nothing: import no folder and light the
        // shelf that holds the name, wherever it hangs — the stored arrival's
        // "oh, that one", answered the way the family gate answers a pick of
        // ground the library already reads.
        cancel_shelf(state);
        reveal::reveal_shelf(state, &ask.existing_id);
        return;
    }
    // The apply runs BEFORE the sheet comes down, and the order is load-bearing
    // rather than tidy: the three answers that import read the interrupted
    // run's root and options off the sheet's ask, so a dismissal first would
    // hand them nothing — an *as new* the reader clicked and a dock that never
    // moved, the sheet closing as the whole of the answer. Both writes land in
    // one tick, so nothing renders between the answer and the dismissal.
    super::apply_placement(state, &placement, answer);
    cancel_shelf(state);
}

// ---------------------------------------------------------------------------
// The shelf half of the unified apply.
// ---------------------------------------------------------------------------
//
// The four answers a shelf question has, each as its own entry point so the
// unified dispatch can reach them. They are the same four writes
// [`answer_shelf`] has always made; what differs is that a book collision asking
// for *replace* now lands on the same rule instead of carrying its own.
//
// Three of the four take the unified ask and do not read it: `root` and `opts`
// are the import the question interrupted, which a [`PlacementAsk`] does not
// carry, because a book collision has no folder to import. They come off the
// shelf sheet's own ask, which is still on the library state while the sheet is
// up. The parameter stays in the signature so the four are one shape and the
// dispatch in `super::apply_placement` does not branch on which of them wants
// what.

/// *Keep both*: mint the arriving folder's shelf under the next free name and
/// import into its own tree.
pub(super) fn as_new_shelf(state: AppState, _ask: &PlacementAsk) {
    // Cloned out of the signal because [`ShelfConflictAsk`] owns the import it
    // interrupted, and an apply that held the signal's borrow across a write
    // would be a second writer of the same list.
    let Some(pending) = state.library.shelf_conflict.ask.with_untracked(|a| a.clone()) else {
        return;
    };
    // Counted at the click rather than at the raise: a shelf that landed between
    // the two is a name the promise has to skip.
    let name = state.library.shelves.with_untracked(|shelves| {
        next_shelf_name(shelves, None, &pending.incoming_name)
    });
    if crate::services::library::import::copies_over_standing_tree(
        state,
        &pending.root,
        &pending.opts,
    ) {
        // A copies run over ground a read-at-place tree still reads is the
        // library's own second instance and no ledger's business: the bound
        // run would resolve onto the tree's row and flip it to copies, which
        // is the *replace*'s conversion rather than the second shelf this
        // answer promised. The unbound run copies the ground onto a shelf of
        // the reader's own, under the counter name, and leaves the tree — its
        // mode, its map, its watch — exactly as the reader's first import made
        // it.
        crate::services::library::import::copies_beside_tree(
            state,
            pending.root,
            pending.opts,
            crate::services::library::import::CopiesDest::NewShelf {
                name,
                after: Some(pending.existing_id),
            },
        );
        return;
    }
    crate::services::library::import::proceed_folder(
        state,
        pending.root,
        pending.opts,
        crate::services::library::import::RootPlan {
            rename: Some(name),
            ..Default::default()
        },
    );
}

/// *Make link*: a pointer row at the shelf, on the level the import would have
/// minted one.
pub(super) fn link_to_shelf(state: AppState, ask: &PlacementAsk, shelf_id: &str) {
    state
        .library
        .add_link(&ask.existing_name, shelf_id, shelf::ALL_SHELF);
    toast(state, format!("Linked to {}.", ask.existing_name));
}

/// *Merge*: the arriving folder IS the shelf that is here — its books join it,
/// and the files whose names it already holds ask one by one on the compact
/// sheet.
pub(super) fn merge_into_shelf(state: AppState, _ask: &PlacementAsk, shelf_id: &str) {
    // Cloned out of the signal because [`ShelfConflictAsk`] owns the import it
    // interrupted, and an apply that held the signal's borrow across a write
    // would be a second writer of the same list.
    let Some(pending) = state.library.shelf_conflict.ask.with_untracked(|a| a.clone()) else {
        return;
    };
    crate::services::library::import::proceed_folder(
        state,
        pending.root,
        pending.opts,
        crate::services::library::import::RootPlan {
            into: Some(shelf_id.to_string()),
            ..Default::default()
        },
    );
}

/// *Replace*: the books the shelf holds leave the library, and the folder's
/// copies take the shelf.
pub(super) fn replace_shelf(state: AppState, _ask: &PlacementAsk, shelf_id: &str) {
    // Cloned out of the signal because [`ShelfConflictAsk`] owns the import it
    // interrupted, and an apply that held the signal's borrow across a write
    // would be a second writer of the same list.
    let Some(pending) = state.library.shelf_conflict.ask.with_untracked(|a| a.clone()) else {
        return;
    };
    // The folder's OWN read-at-place tree is the import module's own sweep: the
    // root's claim first, so a run the reader already started refuses the answer
    // BEFORE anything is removed and a rescan walking the same tree is waited
    // out before anything is removed, then the linked books go and the copy walk
    // spends the logs it wrote. Any other shelf — a stored folder's, a reader's
    // own — is the removal's receipt over the shelf's members and the copies
    // filing into it.
    let own_in_place = pending.own
        && state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .any(|f| f.root == pending.root && f.mode().reads_in_place())
        });
    if own_in_place {
        crate::services::library::import::replace_folder_with_copies(
            state,
            pending.root,
            pending.opts,
        );
    } else {
        crate::services::library::import::replace_shelf_with_folder(
            state,
            pending.root,
            pending.opts,
            shelf_id.to_string(),
        );
    }
}

/// Cancel the folder question: the import simply does not run, which is what
/// Cancel has always meant.
pub fn cancel_shelf(state: AppState) {
    state.library.shelf_conflict.dismiss();
}
