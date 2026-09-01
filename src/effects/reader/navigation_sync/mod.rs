//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)` — `Auto` resolves to a glide,
//!   and the reader's scroll switch is what decides whether it may (see
//!   `scroll_mode`)
//! - mode flip -> scroll: `scroll_to_index(Start, Instant)` when re-entering
//!   continuous mode
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends.
//!
//! A page write that arrives WHILE a transaction holds the geometry (the
//! resume jump on open, an outline click during a fit slide, a search hit
//! mid-gesture) is not dropped, though — it is held by [`JumpGate`] and
//! replayed on the frame the transaction closes. Without that replay, every
//! open from the library lost its resume point: the reader mounts, its
//! container's first measure opens a fit transaction, the resume jump lands
//! one frame later inside it, and the dominant arm "corrected" the page back
//! to 1 — which reading_progress then dutifully saved over the real position.
//!
//! The wiring lives here; the pieces live beside it —
//! [`jump_gate`] holds a navigation across a transaction, [`dominant`] is the
//! scroll → page direction and [`page_to_scroll`] the reverse.
//!
//! INSTALLATION ORDER MATTERS, and it is `page.rs`'s to keep: this must be
//! installed BEFORE `reading_progress`. Leptos runs effects in insertion
//! order, so a transaction closing in the same flush replays its held jump
//! here first, and reading progress then persists the page the reader
//! actually asked for rather than the stale dominant.

mod dominant;
mod jump_gate;
mod page_to_scroll;

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use pdf_core::layout::ViewMode;
use virtual_list_leptos::{Align, Virtualizer};

use crate::state::ReaderState;

use jump_gate::JumpGate;

use super::on_mode_change::on_mode_change;

/// What every arm needs: the state, the shared echo-suppression flag, the
/// mode-restore flag, and the tracked "a zoom transaction is in flight".
///
/// `Copy`-ish by clone: the flags are handles, so an arm gets its own copy of
/// the bag rather than a borrow of a shared one.
#[derive(Clone)]
pub(super) struct Arms {
    pub state: ReaderState,
    /// Echo suppression: a write this module made itself must not be read
    /// back as if the reader had scrolled.
    pub suppress: Rc<Cell<bool>>,
    /// A mode restore is in flight: the scroll→page sync must stand down so it
    /// does not read the not-yet-restored strip and clobber the preserved page.
    ///
    /// A REACTIVE signal on purpose, not a `Cell`. The dominant arm reads it
    /// as a dependency and returns early while it is up; when the restore
    /// lands and it falls, that write re-runs the arm against the now-true
    /// dominant. A `Cell` hand-off left the arm with no reason to re-run after
    /// a mode flip, so the page counter sat on the pre-restore value until the
    /// reader scrolled. Making it reactive closes that without re-introducing
    /// the read-a-strip-mid-restore race the flag exists to prevent.
    pub defer: RwSignal<bool>,
    /// The tracked form of "a zoom transaction is in flight", for the arms
    /// that must RESYNC when one lands. The untracked `zooming_now()` stays on
    /// the ones that must only not fight it.
    pub zooming: Signal<bool>,
}

/// Must be called once from the app root (ReaderPage), alongside the zoom sources.
pub fn navigation_sync(
    state: ReaderState,
    virtualizer: Virtualizer,
    h_virtualizer: Virtualizer,
) {
    let arms = Arms {
        state,
        suppress: Rc::new(Cell::new(false)),
        defer: RwSignal::new(false),
        zooming: state.viewer.zooming(),
    };

    // The mode-restore flag is shared with on_mode_change, which raises it
    // during a mode flip and lowers it once the restored scroll has landed.
    on_mode_change(state, virtualizer.clone(), h_virtualizer.clone(), arms.defer);

    // One gate per axis: its page→scroll arm holds navigation writes that
    // arrive mid-transaction, and its dominant arm defers to the held write
    // on the flush that closes the transaction (see JumpGate).
    let gate = Rc::new(JumpGate::default());
    let h_gate = Rc::new(JumpGate::default());

    dominant::install(
        arms.clone(),
        ViewMode::ScrollVertical,
        virtualizer.clone(),
        gate.clone(),
    );
    page_to_scroll::install(
        arms.clone(),
        ViewMode::ScrollVertical,
        virtualizer,
        gate,
        Align::Start,
    );
    dominant::install(
        arms.clone(),
        ViewMode::ScrollHorizontal,
        h_virtualizer.clone(),
        h_gate.clone(),
    );
    page_to_scroll::install(
        arms,
        ViewMode::ScrollHorizontal,
        h_virtualizer,
        h_gate,
        Align::Center,
    );
}
