//! The `+`/`-` zoom steps.

use leptos::prelude::*;

use pdf_core::math::{nearest_zoom, FitMode};
use crate::effects::reader::zoom::request_zoom;
use crate::state::ReaderState;

/// Applies a manual zoom step and exits fit mode.
///
/// Steps from the zoom currently being aimed at, falling back to the displayed
/// scale when nothing is in flight. Both alternatives are wrong:
///   * `scale` still holds the PREVIOUS committed value during an animation,
///     so a fast `+ +` would resolve to the same preset twice and the second
///     press would do nothing.
///   * `display_scale` is a partway value mid-animation, so `nearest_zoom`
///     would usually return the preset it is already travelling towards —
///     again, a swallowed press.
///
/// Chaining from the in-flight target means each press advances exactly one
/// preset, while the coordinator retargets the running animation from wherever
/// it currently is rather than restarting it.
fn zoom_by(state: ReaderState, dir: i32) {
    let cur = state
        .viewer
        .zoom
        .request
        .get_untracked()
        .filter(|_| state.viewer.zoom_animating.get_untracked())
        .map(|(target, _, _)| target)
        .unwrap_or_else(|| state.viewer.zoom.display.get_untracked());
    state.viewer.fit.set(FitMode::None);
    request_zoom(state, nearest_zoom(cur, dir), true);
}

/// The `+`/`=` and `-`/`_` arms of the keydown dispatch.
pub(super) fn handle_zoom_shortcut(state: ReaderState, ev: &leptos::ev::KeyboardEvent) {
    match ev.key().as_str() {
        "+" | "=" => {
            ev.prevent_default();
            zoom_by(state, 1);
        }
        "-" | "_" => {
            ev.prevent_default();
            zoom_by(state, -1);
        }
        _ => {}
    }
}
