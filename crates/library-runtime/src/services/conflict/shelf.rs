//! The folder's own question, asked before the walk: the level holds the name.

use leptos::prelude::*;

use library_core::conflict::{Arrival, Placement, PlacementAsk, next_shelf_name};
use library_core::folder::FolderOpts;
use library_core::shelf;

use crate::services::reveal;
use crate::services::toast;

/// Its own ask: the answers are about a whole run, not one placement.
#[derive(Clone, PartialEq)]
pub struct ShelfConflictAsk {
    /// The arriving folder's last path segment, the name the reader picked.
    pub incoming_name: String,
    pub existing_id: String,
    pub existing_name: String,
    pub root: String,
    pub opts: FolderOpts,
    /// Whether the named shelf is the folder's own, from a prior run.
    pub own: bool,
}

/// The arrival's mode decides the rows: link, merge or replace.
pub fn offers(ask: &ShelfConflictAsk) -> &'static [Placement] {
    if ask.opts.mode().reads_in_place() {
        Placement::SHELF_READ_IN_PLACE
    } else {
        Placement::SHELF_STORED
    }
}

/// The shelf question on screen; every answer reads it before writing.
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

/// The sheet renders [`offers`] and hands back a [`Placement`]; the write
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
    // Apply before the sheet comes down, and the order is load-bearing.
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
        // The unbound run copies the ground onto a shelf of the reader's own.
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
    // The folder's own read-at-place tree uses the import module's own sweep.
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
