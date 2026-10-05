//! The sheet's *replace*: the books the standing shelf holds leave the library through the
//! removal's own sweep, and the arriving folder's copies take the shelf.

use std::collections::HashSet;

use leptos::prelude::*;

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::ledger;
use library_core::shelf::{self as shelves_ops};

use super::claim::gate_root;
use super::gate::{RootPlan, proceed_folder};
use crate::services::arrange::{self, ReadingData};

/// The books the level's shelf holds leave through the removal's own sweep — rows,
/// memberships, covers, highlights, and the store copies the app made — and the folder's
/// copies take the shelf, the walk filing into it as the merge's plan does.
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
        // The walk that follows lands these files again, as the library's own copies, so the
        // reading data waits for that landing rather than going with these rows.
        arrange::purge_books(state, &doomed, ReadingData::Keep);
    }
    if super::copies::copies_over_standing_tree(state, &root, &opts) {
        // The unbound walk files the copies into the shelf the sweep just
        // emptied and leaves the tree's ledger alone: the bound run would
        // convert the very tree the reader did not ask to convert.
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

/// The linked rows of a read-at-place tree at `root`. A stored book on one
/// of its shelves — a copy that came home — is not among them: the replace
/// is about the instances that read the OS folder.
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

/// Purge the tree's linked rows. The copy import that follows spends their
/// logs as it lands, so the shelf comes back holding only the library's
/// copies, in the names the shelves showed.
pub(crate) fn purge_folder_linked_books(state: crate::context::LibraryContext, root: &str) {
    let doomed = replace_rows_of_tree(state, root);
    if !doomed.is_empty() {
        // The copies that land in the next breath are these files: the data follows them.
        arrange::purge_books(state, &doomed, ReadingData::Keep);
    }
}

/// The check and the claim run in one synchronous step — the webview is
/// single-threaded and nothing awaits between them — so a run the reader
/// already started refuses with the double-import sentence.
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
