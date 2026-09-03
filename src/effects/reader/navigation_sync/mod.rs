//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once from ReaderPage.
//!
//! - scroll -> page: the virtualizer's dominant-page signal
//! - page -> scroll: `scroll_to_index(Start, Auto)` — `Auto` resolves to a glide,
//!   and the reader's scroll switch is what decides whether it may (see
//!   `scroll_mode`)
//! - mount -> scroll: NOT here. A strip that mounts — on a document open, a
//!   return from the library, a switch into its mode — anchors itself to
//!   `viewer.page` in `ScrollShell`, and raises `viewer.awaiting_anchor` until
//!   it has landed. The scroll → page arm stands down for exactly that window,
//!   so the strip's pre-anchor offset (usually the top) can never be read back
//!   as "the reader is on page 1".
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends.
//!
//! A page write that arrives WHILE a transaction holds the geometry (an
//! outline click during a fit slide, a search hit mid-gesture) is not
//! dropped, though — it is held by [`JumpGate`] and replayed on the frame the
//! transaction closes.
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

/// What every arm needs: the state, the shared echo-suppression flag, and
/// the tracked "a zoom transaction is in flight".
///
/// `Copy`-ish by clone: the flags are handles, so an arm gets its own copy of
/// the bag rather than a borrow of a shared one.
#[derive(Clone)]
pub(super) struct Arms {
    pub state: ReaderState,
    /// Echo suppression: a write this module made itself must not be read
    /// back as if the reader had scrolled.
    pub suppress: Rc<Cell<bool>>,
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
        zooming: state.viewer.zooming(),
    };

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
