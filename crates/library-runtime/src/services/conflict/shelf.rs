//! The folder's own question, asked before the walk: the level already holds
//! the name the arriving folder would wear.

use leptos::prelude::*;

use library_core::conflict::{Arrival, Placement, PlacementAsk, next_shelf_name};
use library_core::folder::FolderOpts;
use library_core::shelf;

use crate::services::reveal;
use crate::services::toast;

/// A separate ask rather than a variant of [`ConflictAsk`] because its answers are about
/// a whole import run rather than about one placement: the ones that import start the run
/// again with a plan.
#[derive(Clone, PartialEq)]
pub struct ShelfConflictAsk {
    /// The last segment of the arriving folder's path, the name the reader picked it by.
    pub incoming_name: String,
    pub existing_id: String,
    pub existing_name: String,
    pub root: String,
    pub opts: FolderOpts,
    /// Whether the shelf that holds the name is the arriving folder's OWN — the one its
    /// previous run minted — because a re-import of one folder is a continuation rather than
    /// an arrival, and the sheet words it as one.
    pub own: bool,
}

/// The arrival's MODE decides. A read-at-place arrival gets the pointer and the merge,
/// its *keep both* withheld as the second instance of one ground the family gate exists
/// to prevent, and *replace* with it — neither side a read-at-place collision is the
/// level's to empty.
pub fn offers(ask: &ShelfConflictAsk) -> &'static [Placement] {
    if ask.opts.mode().reads_in_place() {
        Placement::SHELF_READ_IN_PLACE
    } else {
        Placement::SHELF_STORED
    }
}

/// The shelf question on screen. Every answer reads it before writing
/// anything, because the three that import take the interrupted run's root and
/// options off it and the sheet comes down only after the write.
fn pending(state: crate::context::LibraryContext) -> Option<ShelfConflictAsk> {
    state
        .library
        .shelf_conflict
        .ask
        .with_untracked(|a| a.clone())
}

pub fn raise_shelf(state: crate::context::LibraryContext, ask: ShelfConflictAsk) {
    state.library.shelf_conflict.raise(ask);
}

/// The sheet renders [`offers`] and hands back a [`Placement`]; the write is
/// `super::apply_placement` on a shelf scope, the same dispatch a book collision reaches.
pub fn answer_shelf(state: crate::context::LibraryContext, answer: Placement) {
    let Some(ask) = pending(state) else {
        return;
    };
    if !offers(&ask).contains(&answer) {
        return;
    }
    let placement = PlacementAsk::shelf(
        Arrival::folder(ask.incoming_name.clone(), shelf::ALL_SHELF),
        ask.existing_id.clone(),
        ask.existing_name.clone(),
        offers(&ask),
    );
    if answer == Placement::Open {
        cancel_shelf(state);
        reveal::reveal_shelf(state, &ask.existing_id);
        return;
    }
    // The apply runs BEFORE the sheet comes down, and the order is load-bearing: the three
    // answers that import read the interrupted run's root and options off the sheet's ask.
    super::apply_placement(state, &placement, answer);
    cancel_shelf(state);
}

pub(super) fn as_new_shelf(state: crate::context::LibraryContext, _ask: &PlacementAsk) {
    let Some(pending) = pending(state) else {
        return;
    };
    let name = state
        .library
        .shelves
        .with_untracked(|shelves| next_shelf_name(shelves, None, &pending.incoming_name));
    if crate::services::import::copies_over_standing_tree(state, &pending.root, &pending.opts) {
        // The unbound run copies the ground onto a shelf of the reader's own, under the counter
        // name, and leaves the tree alone: the bound run would resolve onto the tree's row and
        // flip it to copies.
        crate::services::import::copies_beside_tree(
            state,
            pending.root,
            pending.opts,
            crate::services::import::CopiesDest::NewShelf {
                name,
                after: Some(pending.existing_id),
            },
        );
        return;
    }
    crate::services::import::proceed_folder(
        state,
        pending.root,
        pending.opts,
        crate::services::import::RootPlan {
            rename: Some(name),
            ..Default::default()
        },
    );
}

pub(super) fn link_to_shelf(
    state: crate::context::LibraryContext,
    ask: &PlacementAsk,
    shelf_id: &str,
) {
    crate::services::arrange::add_link(state, &ask.existing_name, shelf_id, shelf::ALL_SHELF);
    toast(state, format!("Linked to {}.", ask.existing_name));
}

pub(super) fn merge_into_shelf(
    state: crate::context::LibraryContext,
    _ask: &PlacementAsk,
    shelf_id: &str,
) {
    let Some(pending) = pending(state) else {
        return;
    };
    crate::services::import::proceed_folder(
        state,
        pending.root,
        pending.opts,
        crate::services::import::RootPlan {
            into: Some(shelf_id.to_string()),
            ..Default::default()
        },
    );
}

pub(super) fn replace_shelf(
    state: crate::context::LibraryContext,
    _ask: &PlacementAsk,
    shelf_id: &str,
) {
    let Some(pending) = pending(state) else {
        return;
    };
    // The folder's OWN read-at-place tree is the import module's own sweep: the root's claim
    // first, so a run the reader already started refuses the answer before anything is
    // removed.
    let own_in_place = pending.own
        && state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .any(|f| f.root == pending.root && f.mode().reads_in_place())
        });
    if own_in_place {
        crate::services::import::replace_folder_with_copies(state, pending.root, pending.opts);
    } else {
        crate::services::import::replace_shelf_with_folder(
            state,
            pending.root,
            pending.opts,
            shelf_id.to_string(),
        );
    }
}

pub fn cancel_shelf(state: crate::context::LibraryContext) {
    state.library.shelf_conflict.dismiss();
}
