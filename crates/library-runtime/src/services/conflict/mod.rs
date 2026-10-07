//! The "already imported?" sheet: the wiring between the pure rule and
//! the reader's answers.

mod covered;
mod folder_merge;
mod name;
mod note;
pub(crate) mod shelf;

#[cfg(test)]
mod tests;

pub use covered::answer_covered;
pub use folder_merge::answer_folder_merge;
pub use name::answer_placement;
pub use note::{close_already_imported, raise_note};
pub use shelf::{
    ShelfConflictAsk, answer_shelf, cancel_shelf, offers as shelf_offers, raise_shelf,
};

use leptos::prelude::*;

use library_core::book::{Row, find_row};
use library_core::conflict::{Arrival, Placement, PlacementAsk, Scope, collide, next_name};
use library_core::folder::FolderMode;

use crate::context::LibraryContext;

/// Which question an ask is: three sheets share one queue and one signal.
#[derive(Clone, PartialEq)]
pub enum AskKind {
    /// Which three answers is the arrival's fact; this variant carries nothing.
    NameCollision,
    /// A per-file question from a folder import merging into a held shelf.
    FolderMerge {
        /// A read-in-place folder lands a link now; a copying one, later.
        mode: FolderMode,
        /// The folder whose ledger records it, so a rescan stays quiet.
        folder_id: String,
    },
    /// Two answers: the stored copy here, or the folder's book lit.
    Covered {
        /// The folder whose tree holds the file; the ask exists for one.
        folder_id: String,
    },
    /// The library's question, not the level's: this book is held somewhere.
    AlreadyHave,
}

impl AskKind {
    /// Both folder kinds have one, so a later rescan stays quiet.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            AskKind::NameCollision | AskKind::AlreadyHave => None,
            AskKind::FolderMerge { folder_id, .. } => Some(folder_id.as_str()),
            AskKind::Covered { folder_id } => Some(folder_id),
        }
    }

    /// `false` for the kinds with no folder: a collision copy is the library's.
    pub fn reads_in_place(&self) -> bool {
        matches!(self, AskKind::FolderMerge { mode, .. } if mode.reads_in_place())
    }

    /// Whether this is the level's name question; the waiting count uses it.
    pub fn is_name_question(&self) -> bool {
        matches!(self, AskKind::NameCollision)
    }

    pub fn is_folder_merge(&self) -> bool {
        matches!(self, AskKind::FolderMerge { .. })
    }

    /// The covered shape, shared with the library's content question.
    pub fn is_two_answer(&self) -> bool {
        matches!(self, AskKind::Covered { .. } | AskKind::AlreadyHave)
    }
}

#[derive(Clone, PartialEq)]
pub struct ConflictAsk {
    /// Kept whole: an answer places it with its file or row and level.
    pub arrival: Arrival,
    pub existing_id: String,
    /// Read once: heading and buttons must not each derive it.
    pub existing_name: String,
    pub kind: AskKind,
}

impl ConflictAsk {
    fn name_collision(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::NameCollision,
        }
    }

    pub fn folder_merge(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        mode: FolderMode,
        folder_id: String,
    ) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::FolderMerge { mode, folder_id },
        }
    }

    pub fn already_have(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::AlreadyHave,
        }
    }

    pub fn covered(
        arrival: Arrival,
        existing_id: String,
        existing_name: String,
        folder_id: String,
    ) -> Self {
        Self {
            arrival,
            existing_id,
            existing_name,
            kind: AskKind::Covered { folder_id },
        }
    }
}

impl ConflictAsk {
    /// One function for both scopes: the sheets differ only in answers offered.
    pub fn placement(&self, state: crate::context::LibraryContext) -> PlacementAsk {
        let offers = match &self.kind {
            AskKind::Covered { .. } | AskKind::AlreadyHave => Placement::COVERED,
            AskKind::NameCollision => offers_for(state, self),
            AskKind::FolderMerge { .. } => Placement::FOLDER_MERGE,
        };
        PlacementAsk::book(
            self.arrival.clone(),
            self.existing_id.clone(),
            self.existing_name.clone(),
            offers,
        )
    }
}

/// The single dispatch the per-kind apply functions once each wrote half of.
pub fn apply_placement(
    state: crate::context::LibraryContext,
    ask: &PlacementAsk,
    choice: Placement,
) {
    if !ask.offers_placement(choice) {
        return;
    }
    match choice {
        Placement::Open => crate::services::reveal::reveal_book(state, ask.existing.id()),
        Placement::KeepBoth => keep_both(state, ask),
        Placement::LinkOnly => link_to(state, ask),
        Placement::Merge => merge_into(state, ask),
        Placement::Replace => replace_with(state, ask),
    }
}

fn keep_both(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::as_new_placement(state, ask),
        Scope::Shelf { .. } => shelf::as_new_shelf(state, ask),
    }
}

fn link_to(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { row_id } => name::link_to_row(state, ask, row_id),
        Scope::Shelf { shelf_id } => shelf::link_to_shelf(state, ask, shelf_id),
    }
}

fn merge_into(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::merge_into_row(state, ask),
        Scope::Shelf { shelf_id } => shelf::merge_into_shelf(state, ask, shelf_id),
    }
}

fn replace_with(state: crate::context::LibraryContext, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::replace_row(state, ask),
        Scope::Shelf { shelf_id } => shelf::replace_shelf(state, ask, shelf_id),
    }
}

/// A read-at-place row dragged onto the library's own copy: *make link*.
fn link_shape(state: crate::context::LibraryContext, ask: &ConflictAsk) -> bool {
    let Some(moved_id) = ask.arrival.moving.as_deref() else {
        return false;
    };
    let existing_is_a_copy = state.library.books.with_untracked(|rows| {
        find_row(rows, &ask.existing_id)
            .and_then(|row| row.book())
            .is_some_and(|book| book.origin.is_stored())
    });
    existing_is_a_copy
        && crate::services::arrange::converts_on_move_to(state, moved_id, &ask.arrival.shelf_id)
}

/// One function, so sheet and answer cannot drift about the buttons.
pub fn offers_for(
    state: crate::context::LibraryContext,
    ask: &ConflictAsk,
) -> &'static [Placement] {
    if ask.arrival.is_import() {
        Placement::FILE
    } else if link_shape(state, ask) {
        Placement::MOVE_KEEPING_BOTH
    } else {
        Placement::MOVE
    }
}

/// One spelling: the heading and every button print this name.
pub(crate) fn existing_name_of(rows: &[Row], existing_id: &str, arrival: &Arrival) -> String {
    find_row(rows, existing_id)
        .map(|row| row.display_name())
        .unwrap_or_else(|| arrival.name.clone())
}

/// Every placing surface screens through here before writing.
pub fn screen(
    state: crate::context::LibraryContext,
    arrivals: Vec<Arrival>,
) -> (Vec<Arrival>, Vec<ConflictAsk>) {
    let (rows, shelves) = state.library.snapshot_rows();
    let mut clean = Vec::with_capacity(arrivals.len());
    let mut asks = Vec::new();
    for arrival in arrivals {
        match collide(&rows, &shelves, &arrival) {
            Some(existing_id) => {
                let existing_name = existing_name_of(&rows, &existing_id, &arrival);
                asks.push(ConflictAsk::name_collision(
                    arrival,
                    existing_id,
                    existing_name,
                ));
            }
            None => clean.push(arrival),
        }
    }
    (clean, asks)
}

/// A sheet already up queues new asks: two drops owe two answers.
pub fn raise(state: crate::context::LibraryContext, asks: Vec<ConflictAsk>) {
    if asks.is_empty() {
        return;
    }
    let open = state.library.conflict.open.get_untracked();
    if open && state.library.conflict.ask.get_untracked().is_some() {
        state.library.conflict_waiting.update(|waiting| {
            waiting.extend(asks);
        });
        return;
    }
    let mut asks = asks;
    let first = asks.remove(0);
    state.library.conflict_waiting.update(|waiting| {
        waiting.extend(asks);
    });
    state.library.conflict.raise(first);
}

pub(super) fn advance(state: crate::context::LibraryContext) {
    let next = state
        .library
        .conflict_waiting
        .with_untracked(|waiting| waiting.first().cloned());
    match next {
        Some(ask) => {
            state.library.conflict_waiting.update(|waiting| {
                waiting.remove(0);
            });
            state.library.conflict.ask.set(Some(ask));
        }
        None => cancel(state),
    }
}

/// Skips the question on screen and all behind it; answers already given stay.
pub fn cancel(state: crate::context::LibraryContext) {
    state.library.conflict.dismiss();
    state.library.conflict_waiting.set(Vec::new());
}

/// Read at the click, once: both name-minting answers mint the same one.
pub(super) fn minted_name(state: crate::context::LibraryContext, ask: &ConflictAsk) -> String {
    let (rows, shelves) = state.library.snapshot_rows();
    next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

pub(super) fn member_slot(
    state: crate::context::LibraryContext,
    shelf_id: &str,
    row_id: &str,
) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        library_core::shelf::find(shelves, shelf_id)
            .and_then(|s| s.books.iter().position(|m| m == row_id))
    })
}

/// One spelling for both sheets' drain: a question of the other kind stops it.
pub(super) fn answer_batch<A: Copy + 'static>(
    state: crate::context::LibraryContext,
    answer: A,
    apply_all: bool,
    is_mine: fn(&AskKind) -> bool,
    apply: fn(LibraryContext, &ConflictAsk, A),
) {
    let Some(ask) = state.library.conflict.ask.get_untracked() else {
        return;
    };
    if !is_mine(&ask.kind) {
        return;
    }
    apply(state, &ask, answer);
    advance(state);
    if !apply_all {
        return;
    }
    while let Some(next) = state.library.conflict.ask.get_untracked() {
        if !is_mine(&next.kind) {
            break;
        }
        apply(state, &next, answer);
        advance(state);
    }
}
