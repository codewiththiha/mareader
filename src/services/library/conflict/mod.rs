//! The "already imported?" sheet: one question, three answers.
//!
//! The RULE is not here — it is `library_core::conflict`, which is pure and
//! host-tested. This file is the wiring between that answer and the three things a
//! reader can do about it.

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
    answer_shelf, cancel_shelf, offers as shelf_offers, raise_shelf, ShelfConflictAsk,
};

use leptos::prelude::*;

use library_core::book::{find_row, Row};
use library_core::conflict::{collide, next_name, Arrival, Placement, PlacementAsk, Scope};
use library_core::folder::FolderMode;

use crate::state::AppState;

/// Which question an ask is, and the facts only that question has: three sheets share
/// one queue and one signal, and two of the four answer functions used to open with a
/// runtime guard over three booleans.
#[derive(Clone, PartialEq)]
pub enum AskKind {
    /// WHICH three answers the sheet offers is the arrival's fact rather than this one's, so this variant carries nothing.
    NameCollision,
    /// A per-file question out of a folder import merging into a shelf the level already held.
    FolderMerge {
        /// A folder that reads in place lands a linked answer now, a copying one lands it after its copy.
        mode: FolderMode,
        /// The watched folder whose ledger records the placement when the answer lands, so a later rescan stays quiet.
        folder_id: String,
    },
    /// Two answers rather than three — the library's own stored copy on this level, or the
    /// folder's book lit where it stands — because a second row of one linked file is the one
    /// thing a read-at-place folder can never make.
    Covered {
        /// The folder whose tree holds the file. Always one: the ask exists because a specific tree covers the ground.
        folder_id: String,
    },
    /// The question is the library's rather than the level's: the reader already has this
    /// book, somewhere, and what is asked is whether they meant to add a second instance or
    /// to go to the one they have.
    AlreadyHave,
}

impl AskKind {
    /// Both folder kinds have one: a placement the ledger does not know about is a book the next rescan adds again.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            AskKind::NameCollision | AskKind::AlreadyHave => None,
            AskKind::FolderMerge { folder_id, .. } => Some(folder_id.as_str()),
            AskKind::Covered { folder_id } => Some(folder_id),
        }
    }

    /// `false` for the two kinds that have no folder of their own: a name collision's copy is the library's own whatever the level is.
    pub fn reads_in_place(&self) -> bool {
        matches!(self, AskKind::FolderMerge { mode, .. } if mode.reads_in_place())
    }

    /// Whether this is the level's own NAME question — the one the three-answer sheet
    /// renders. The waiting count behind that sheet counts only these: "3 more waiting"
    /// that included a covered file's two-answer question would promise a batch the
    /// sheet's own answers can never consume.
    pub fn is_name_question(&self) -> bool {
        matches!(self, AskKind::NameCollision)
    }

    pub fn is_folder_merge(&self) -> bool {
        matches!(self, AskKind::FolderMerge { .. })
    }

    /// The covered file's shape, which the library's content question shares. One predicate rather than two, because the two render the same rows.
    pub fn is_two_answer(&self) -> bool {
        matches!(self, AskKind::Covered { .. } | AskKind::AlreadyHave)
    }
}

#[derive(Clone, PartialEq)]
pub struct ConflictAsk {
    /// Kept whole: an answer places it, and a placement needs the file or the row and the level it was going to.
    pub arrival: Arrival,
    pub existing_id: String,
    /// Read once: the sheet prints it in three places, and a heading and two buttons must not each derive their own.
    pub existing_name: String,
    pub kind: AskKind,
}

impl ConflictAsk {
    pub fn name_collision(arrival: Arrival, existing_id: String, existing_name: String) -> Self {
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
    /// One function rather than one per sheet, because the sheets differ only in WHICH answers
    /// they offer and in what the thing already there is — a row or a shelf — and both of those
    /// are already on the ask.
    pub fn placement(&self, state: AppState) -> PlacementAsk {
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

/// The single dispatch the five per-kind apply functions used to each write their own half
/// of. `Merge` and `Replace` are where the duplication cost anything — each re-derived how to
/// purge the loser, fold the progress and re-seat the membership — and they now branch once.
pub fn apply_placement(state: AppState, ask: &PlacementAsk, choice: Placement) {
    if !ask.offers_placement(choice) {
        return;
    }
    match choice {
        Placement::Open => crate::services::library::reveal::reveal_book(state, ask.existing.id()),
        Placement::KeepBoth => keep_both(state, ask),
        Placement::LinkOnly => link_to(state, ask),
        Placement::Merge => merge_into(state, ask),
        Placement::Replace => replace_with(state, ask),
    }
}

fn keep_both(state: AppState, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::as_new_placement(state, ask),
        Scope::Shelf { .. } => shelf::as_new_shelf(state, ask),
    }
}

fn link_to(state: AppState, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { row_id } => name::link_to_row(state, ask, row_id),
        Scope::Shelf { shelf_id } => shelf::link_to_shelf(state, ask, shelf_id),
    }
}

fn merge_into(state: AppState, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::merge_into_row(state, ask),
        Scope::Shelf { shelf_id } => shelf::merge_into_shelf(state, ask, shelf_id),
    }
}

fn replace_with(state: AppState, ask: &PlacementAsk) {
    match &ask.existing {
        Scope::Book { .. } => name::replace_row(state, ask),
        Scope::Shelf { shelf_id } => shelf::replace_shelf(state, ask, shelf_id),
    }
}

/// The row being dragged is a read-at-place book an in-place folder placed, and the row
/// already on the level is one of the library's own stored copies: neither side is the
/// reader's to destroy, so the sheet offers *make link* in place of *replace*.
pub fn link_shape(state: AppState, ask: &ConflictAsk) -> bool {
    let Some(moved_id) = ask.arrival.moving.as_deref() else {
        return false;
    };
    let existing_is_a_copy = state.library.books.with_untracked(|rows| {
        find_row(rows, &ask.existing_id)
            .and_then(|row| row.book())
            .is_some_and(|book| book.origin.is_stored())
    });
    existing_is_a_copy
        && crate::services::library::arrange::converts_on_move_to(
            state,
            moved_id,
            &ask.arrival.shelf_id,
        )
}

/// One function rather than a branch in the sheet and a second in the answer, so the two
/// cannot drift about which buttons a given arrival gets.
pub fn offers_for(state: AppState, ask: &ConflictAsk) -> &'static [Placement] {
    if ask.arrival.is_import() {
        Placement::FILE
    } else if link_shape(state, ask) {
        Placement::MOVE_KEEPING_BOTH
    } else {
        Placement::MOVE
    }
}

/// One spelling, because the sheet prints this name in its heading and in every button's
/// sentence: a site that derived its own would eventually disagree with the others about
/// which book the question is about.
pub(crate) fn existing_name_of(rows: &[Row], existing_id: &str, arrival: &Arrival) -> String {
    find_row(rows, existing_id)
        .map(|row| row.display_name())
        .unwrap_or_else(|| arrival.name.clone())
}

/// Every placing surface hands its placements through here before writing
/// anything, and applies the clean half at once.
pub fn screen(state: AppState, arrivals: Vec<Arrival>) -> (Vec<Arrival>, Vec<ConflictAsk>) {
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

/// A sheet already up takes new asks onto its queue rather than being
/// replaced: two drops in flight owe two answers.
pub fn raise(state: AppState, asks: Vec<ConflictAsk>) {
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

pub(super) fn advance(state: AppState) {
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

/// Skips the question on screen and every one behind it; placements already
/// answered keep their answers.
pub fn cancel(state: AppState) {
    state.library.conflict.dismiss();
    state.library.conflict_waiting.set(Vec::new());
}

/// Read at the click rather than at the raise, in one place: the two answers
/// that mint a name must mint the same one for the same arrival.
pub(super) fn minted_name(state: AppState, ask: &ConflictAsk) -> String {
    let (rows, shelves) = state.library.snapshot_rows();
    next_name(&rows, &shelves, &ask.arrival.shelf_id, &ask.arrival.name)
}

pub(super) fn member_slot(state: AppState, shelf_id: &str, row_id: &str) -> Option<usize> {
    state.library.shelves.with_untracked(|shelves| {
        library_core::shelf::find(shelves, shelf_id)
            .and_then(|s| s.books.iter().position(|m| m == row_id))
    })
}

/// One spelling rather than one per sheet because the two sheets' contract is one contract,
/// and the half of it that is easy to get wrong is the stop: a question of the other kind
/// belongs to another gesture.
pub(super) fn answer_batch<A: Copy + 'static>(
    state: AppState,
    answer: A,
    apply_all: bool,
    is_mine: fn(&AskKind) -> bool,
    apply: fn(AppState, &ConflictAsk, A),
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
