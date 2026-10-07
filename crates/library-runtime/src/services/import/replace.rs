//! The sheet's *replace*: the shelf's books leave, the copies take it.

use std::collections::HashSet;

use leptos::prelude::*;

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::ledger;
use library_core::shelf::{self as shelves_ops};

use super::claim::gate_root;
use super::gate::{RootPlan, proceed_folder};
use crate::services::arrange::{self, ReadingData};

/// The level's books leave by the removal's sweep; the copies take it.
pub(crate) fn replace_shelf_with_folder(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    existing_id: String,
) {
    let walk_root = root.clone();
    gate_root(state, &root, move || {
        sweep_and_walk_into(state, walk_root, opts, existing_id)
    });
}

fn sweep_and_walk_into(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    existing_id: String,
) {
    let doomed: Vec<String> = {
        let (rows, shelves) = state.library.snapshot_rows();
        shelves_ops::members_of(&rows, &shelves, &existing_id)
            .into_iter()
            .map(str::to_string)
            .collect()
    };
    if !doomed.is_empty() {
        // The walk re-lands these files: the reading data waits for it.
        arrange::purge_books(state, &doomed, ReadingData::Keep);
    }
    if super::copies::copies_over_standing_tree(state, &root, &opts) {
        // The unbound walk leaves the tree's ledger alone, as asked.
        super::copies::copies_beside_tree(
            state,
            root,
            opts,
            super::copies::CopiesDest::Into {
                shelf_id: existing_id,
            },
        );
        return;
    }
    proceed_folder(
        state,
        root,
        opts,
        RootPlan {
            into: Some(existing_id),
            ..Default::default()
        },
    );
}

/// The tree's linked rows; a stored copy on one is not among them.
pub fn replace_rows_of_tree(state: crate::context::LibraryContext, root: &str) -> Vec<String> {
    let placed: HashSet<Fingerprint> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .find(|f| f.root == root && f.mode().reads_in_place())
            .map(|f| f.placed.clone())
            .unwrap_or_default()
    });
    if placed.is_empty() {
        return Vec::new();
    }
    state
        .library
        .books
        .with_untracked(|rows| ledger::linked_rows_of(rows, &placed))
}

/// Purge the tree's linked rows; the copy import spends their logs.
pub(crate) fn purge_folder_linked_books(state: crate::context::LibraryContext, root: &str) {
    let doomed = replace_rows_of_tree(state, root);
    if !doomed.is_empty() {
        // These files land again: the data follows them.
        arrange::purge_books(state, &doomed, ReadingData::Keep);
    }
}

/// The check and the claim are one synchronous step.
pub(crate) fn replace_folder_with_copies(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
) {
    let walking = root.clone();
    gate_root(state, &root, move || {
        purge_folder_linked_books(state, &walking);
        proceed_folder(state, walking, opts, RootPlan::default());
    });
}
