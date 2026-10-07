//! The mount anchor's settle loop, shared by every strip.

use std::rc::Rc;

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::state::ReaderState;

/// Aim a freshly mounted strip at the reader's position.
pub(crate) fn settle(state: ReaderState, v: &Virtualizer, frames: u32, aim: impl Fn() + 'static) {
    let generation = state.viewer.begin_anchor();
    // Shared, not borrowed: the loop re-arms itself.
    let aim: Rc<dyn Fn()> = Rc::new(aim);
    frame(state, v.clone(), frames, generation, aim);
}

fn frame(state: ReaderState, v: Virtualizer, frames_left: u32, generation: u64, aim: Rc<dyn Fn()>) {
    // The surface can outlive the reader state; probe first.
    if state.viewer.awaiting_anchor.try_get_untracked().is_none() {
        return;
    }
    if !state.viewer.owns_anchor(generation) {
        return;
    }
    v.remeasure_viewport();
    aim();

    if landed(&v) {
        release(state, generation, true);
        return;
    }
    if frames_left == 0 {
        release(state, generation, false);
        return;
    }
    request_animation_frame(move || {
        if !state.viewer.owns_anchor(generation) {
            return;
        }
        // A detached surface has nothing to anchor.
        if !v.is_bound() {
            release(state, generation, false);
            return;
        }
        frame(state, v, frames_left - 1, generation, aim.clone());
    });
}

/// Landed = the browser holds the offset the core adopted.
fn landed(v: &Virtualizer) -> bool {
    let core = v.scroll_offset().get_untracked();
    v.surface_offset()
        .is_some_and(|dom| (dom - core).abs() <= 1.0)
        && v.viewport().get_untracked().main > 1.0
}

/// Lower the guard the scroll→page sync stands behind on mount.
fn release(state: ReaderState, generation: u64, painted: bool) {
    if state.viewer.owns_anchor(generation) {
        state.viewer.awaiting_anchor.set(false);
        if painted && state.reflowable() && !state.viewer.first_paint.get_untracked() {
            state.viewer.first_paint.set(true);
        }
    }
}
