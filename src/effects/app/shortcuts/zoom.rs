//! The `+`/`-` zoom steps.

use crate::state::reader::ZoomCommand;
use crate::state::ReaderState;

/// Applies a manual zoom step. A plain command post: the controller resolves
/// the step (chaining from an in-flight transition's target so a fast `+ +`
/// advances two presets, never swallowing the second press), clears the fit
/// mode and runs the one transition pipeline.
fn zoom_by(state: ReaderState, dir: i32) {
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
