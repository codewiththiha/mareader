//! The web test hook: query-parameter document opens and blend toggles for
//! the browser-level lifecycle suite (`tests/browser/`, Deep CI).
//!
//! Narrowly scoped by design (Phase 0 Rule A allows test hooks; AGENTS.md
//! forbids unbounded fallbacks):
//!
//! - It is INERT in the packaged app — the whole hook returns immediately
//!   when a Tauri webview is attached, so a shipped binary ignores query
//!   parameters entirely.
//! - `open` only accepts web-served `/samples/` paths, so the hook can open
//!   exactly the fixtures the repository ships, never an arbitrary local
//!   file.
//! - `blend` flips `settings.layout.blend_mode` through the app's own
//!   signal — the same write the appearance menu makes — so the paper
//!   session, the scroll-tick position feeds and the look-ahead all run the
//!   real production path, not a parallel one.
//!
//! Both parameters ride the ordinary open/configure flows; nothing here is
//! a second implementation of either.

use crate::state::AppState;

/// Install the hook. Called once from the app root, after the app effects
/// exist (an `open` claim may run the reader route the moment it lands).
pub fn init(state: AppState) {
    #[cfg(target_arch = "wasm32")]
    init_web(state);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
}

#[cfg(target_arch = "wasm32")]
fn init_web(state: AppState) {
    // The signal write below goes through the `Update` trait; the import
    // lives here because this whole fn (and thus the need for it) is
    // wasm-only — a host build would call the import unused.
    use leptos::prelude::Update;

    // The packaged app never answers query parameters.
    if tauri_bridge::has_tauri() {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(search) = window.location().search() else {
        return;
    };
    let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search) else {
        return;
    };

    if params.get("blend").is_some() {
        // The appearance menu's write, through the state the boot effects
        // publish from: the paper session reconfigures on the tracked
        // change, and the scroll ticks feed the look-ahead from then on.
        state.settings.update(|s| s.layout.blend_mode = true);
    }

    if let Some(path) = params.get("open") {
        // Samples only: the fixtures the repo ships, served by the same
        // static server the browser build uses.
        if path.starts_with("/samples/") {
            crate::services::document::open_path(state, path);
        }
    }
}
