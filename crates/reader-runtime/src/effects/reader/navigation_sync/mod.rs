//! Navigation sync: keeps `viewer.page` and the continuous/horizontal scroll
//! position in sync around the virtualizers. Wired once per pane, from its mount.
//!
//! - scroll → page: the virtualizer's dominant-page signal ([`dominant`]);
//! - page → scroll: `scroll_to_index(Start, Auto)` — `Auto` resolves to a
//!   glide, and the reader's scroll switch decides whether it may
//!   ([`page_to_scroll`]);
//! - mount → scroll: NOT here. A mounting strip anchors itself to
//!   `viewer.page` in `ScrollShell` and raises `viewer.awaiting_anchor` until
//!   landed; the scroll → page arm stands down for exactly that window, so
//!   the strip's pre-anchor offset can never be read back as "page 1".
//!
//! Both directions stand down while a zoom transaction is in flight: a zoom
//! moves the geometry, and the transaction's anchor — not a window churn's
//! idea of the dominant item — decides where the reader lands. Sync resumes
//! against the committed geometry when the transition ends. A page write that
//! arrives WHILE a transaction holds the geometry (an outline click during a
//! fit slide, a search hit mid-gesture) is not dropped: [`JumpGate`] holds it
//! and replays it on the frame the transaction closes.
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

use reader_core::view::ViewMode;
use virtual_list_leptos::{Align, ScrollMode, Virtualizer};

use crate::state::ReaderState;

use jump_gate::JumpGate;

/// How a jump the reader commanded should travel: gliding while the reader's
/// scroll switch allows it, in one step when it does not. Read UNTRACKED,
/// because the flag that stops a jump gliding must not be what re-runs the
/// jump.
///
/// Shared by both commands that land on a virtualizer item — the page write
/// ([`page_to_scroll`]) and the outline's chapter jump
/// (`effects::reader::outline_jump`) — so the reader's motion setting means
/// one thing, not one thing per command.
pub(crate) fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

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

/// Must be called once per pane (its mount), alongside the zoom sources.
pub fn navigation_sync(state: ReaderState, virtualizer: Virtualizer, h_virtualizer: Virtualizer) {
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
