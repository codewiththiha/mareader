//! The Blend handshake, reader side: the host broadcasts the shared paper to
//! every pane, and each pane paints it the moment the command lands instead
//! of waiting for a scroll to invalidate the surface.
//!
//! Two halves, at two levels: [`apply`] rides the runtime's command listener
//! (it is the writer — the epoch and the root CSS state move together,
//! synchronously, in the frame the command arrives), and [`install`] wires the
//! surface effect that re-asserts the paper whenever the epoch moves, so a
//! settings commit that lands a beat later cannot leave the backdrop half
//! updated.

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::state::AppState;
use crate::state::AppearanceSignal;

/// The host sends the detected paper as `#rrggbb`; anything else is "blend is
/// off", not a colour to paint.
fn valid_paper(paper: &str) -> bool {
    paper.len() == 7 && paper.starts_with('#') && paper[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

/// One set-blend command: store the shared paper, bump the epoch, and force
/// the root CSS state so the surface repaints now.
pub fn apply(state: AppState, paper: Option<&str>) {
    state
        .reader
        .blend_paper
        .set(paper.filter(|p| valid_paper(p)).map(str::to_string));
    state.reader.blend_epoch.update(|epoch| *epoch += 1);
    paint(state);
}

/// The root CSS state the blend paper lives in: the two paper custom
/// properties the surfaces read, plus the epoch attribute that makes the
/// update observable from outside the reactive graph.
fn paint(state: AppState) {
    let Some(element) =
        web_sys::window().and_then(|w| w.document()).and_then(|d| d.document_element())
    else {
        return;
    };
    // The Leptos element extension shadows the web `style` accessor, so the
    // paper goes through the DOM element it belongs to.
    let root: web_sys::HtmlElement = element.unchecked_into();
    let style = root.style();
    match state.reader.blend_paper.get() {
        Some(color) => {
            let _ = style.set_property("--color-paper", &color);
            let _ = style.set_property("--tx-paper", &color);
            let _ = root.set_attribute(
                "data-blend-epoch",
                &state.reader.blend_epoch.get_untracked().to_string(),
            );
        }
        // Blend off: the theme pipeline owns the paper again, so the
        // properties stay put — only the marker goes away.
        None => {
            let _ = root.remove_attribute("data-blend-epoch");
        }
    }
}

/// The surface half: a blend update is a reactive event the reader root
/// answers, not a scroll that eventually finds it.
pub fn install(state: AppState) {
    let appearance = expect_context::<AppearanceSignal>();
    Effect::new(move |_| {
        let epoch = state.reader.blend_epoch.get();
        if epoch == 0 {
            return;
        }
        // Read both so the effect re-asserts when either moves: a blend
        // command, or the settings commit that carries the shared
        // appearance a beat behind it.
        let _ = state.reader.blend_paper.get();
        let _ = appearance.get();
        paint(state);
    });
}
