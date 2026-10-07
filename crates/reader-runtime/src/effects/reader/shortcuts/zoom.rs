//! The `+`/`-` zoom steps.

use crate::state::ReaderState;
use crate::state::ZoomCommand;

/// Applies a manual zoom step through the one transition pipeline.
pub(super) fn zoom_by(state: ReaderState, dir: i32) {
    state.viewer.zoom.post(ZoomCommand::Step(dir), true);
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
