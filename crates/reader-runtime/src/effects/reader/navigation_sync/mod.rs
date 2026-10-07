//! Navigation sync: keeps `viewer.page` and the scroll in step.

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

/// How a commanded jump travels: gliding, or in one step.
pub(crate) fn scroll_mode(state: ReaderState) -> ScrollMode {
    if state.viewer.motion.get_untracked().scroll_glide {
        ScrollMode::Auto
    } else {
        ScrollMode::Instant
    }
}

/// What every arm needs: state, echo flag, zoom flag.
#[derive(Clone)]
pub(super) struct Arms {
    pub state: ReaderState,
    /// Echo suppression: our own write is not a reader scroll.
    pub suppress: Rc<Cell<bool>>,
    /// The tracked form of "a zoom transaction is in flight".
    pub zooming: Signal<bool>,
}

/// Must be called once per pane (its mount), alongside the zoom sources.
pub fn navigation_sync(state: ReaderState, virtualizer: Virtualizer, h_virtualizer: Virtualizer) {
    let arms = Arms {
        state,
        suppress: Rc::new(Cell::new(false)),
        zooming: state.viewer.zooming(),
    };

    // One gate per axis; its arms hold and replay (see JumpGate).
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
