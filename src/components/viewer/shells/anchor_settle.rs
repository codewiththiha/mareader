//! The mount anchor's settle loop, shared by every strip that has to place
//! itself on a fresh mount.
//!
//! A freshly mounted strip is put on the reader's position by writing a scroll
//! offset the moment its container binds — and the browser may not have laid
//! that container out yet. A `scrollTop` written into a box that is still 0
//! tall is silently clamped, so the write is re-asserted for a few frames and
//! checked against the DOM each time. That machinery is identical for the page
//! strips and for the reflowable stream; what differs is only WHERE a frame
//! aims (a page index against an axis, or a saved fraction of the stream's
//! extent) and how many frames the surface is given.
//!
//! Both callers keep their own budget, and the difference is deliberate: the
//! stream is aiming at a fraction of a total that is still growing while its
//! blocks report measured heights, so it needs more attempts before the offset
//! it wrote means what it will mean once the layout settles. A page strip is
//! aiming at a page of known size.
//!
//! The loop hands the reader back to the scroll→page sync by lowering
//! `viewer.awaiting_anchor` exactly once, on whichever comes first: the DOM
//! agreeing with the core, the budget running out, or the surface detaching.
//! The generation token (`state.viewer.begin_anchor` / `owns_anchor`) is what
//! makes a superseded loop stop touching a strip that is no longer the reader's.

use std::rc::Rc;

use leptos::prelude::*;
use virtual_list_leptos::Virtualizer;

use crate::state::ReaderState;

/// Aim a freshly mounted strip at the reader's position, re-asserting for up to
/// `frames` frames until the DOM agrees.
///
/// `aim` runs on EVERY frame, not just the first: the position can move while
/// the surface is settling (a navigation issued mid-settle wins over the value
/// the mount started with, and the stream's saved fraction is consumed by the
/// first frame that can use it).
///
/// The virtualizer is borrowed because the aim usually needs one too: a caller
/// hands the loop a handle and a closure holding another handle to the same
/// surface, and taking the first by value would leave nothing to close over.
pub(crate) fn settle(state: ReaderState, v: &Virtualizer, frames: u32, aim: impl Fn() + 'static) {
    let generation = state.viewer.begin_anchor();
    // Shared rather than borrowed: the loop re-arms itself from inside a
    // `request_animation_frame` callback, which has to OWN everything it runs.
    let aim: Rc<dyn Fn()> = Rc::new(aim);
    frame(state, v.clone(), frames, generation, aim);
}

fn frame(state: ReaderState, v: Virtualizer, frames_left: u32, generation: u64, aim: Rc<dyn Fn()>) {
    if !state.viewer.owns_anchor(generation) {
        return;
    }
    v.remeasure_viewport();
    aim();

    if frames_left == 0 || landed(&v) {
        release(state, generation);
        return;
    }
    request_animation_frame(move || {
        if !state.viewer.owns_anchor(generation) {
            return;
        }
        // The strip may have been unmounted (a close, a mode flip) between
        // frames; a detached surface has nothing to anchor.
        if !v.is_bound() {
            release(state, generation);
            return;
        }
        frame(state, v, frames_left - 1, generation, aim.clone());
    });
}

/// Landed = the browser holds the offset the core adopted, inside a box that
/// has a real extent. A clamped write leaves the two apart.
fn landed(v: &Virtualizer) -> bool {
    let core = v.scroll_offset().get_untracked();
    v.surface_offset()
        .is_some_and(|dom| (dom - core).abs() <= 1.0)
        && v.viewport().get_untracked().main > 1.0
}

/// Lower the guard the scroll→page sync stands behind while a mount anchors.
fn release(state: ReaderState, generation: u64) {
    if state.viewer.owns_anchor(generation) {
        state.viewer.awaiting_anchor.set(false);
    }
}
