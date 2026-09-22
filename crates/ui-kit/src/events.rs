//! The app's window-`CustomEvent` protocol: every event name in one table,
//! plus the typed dispatchers.
//!
//! Window CustomEvents are the app's cross-cutting message mechanism — they
//! cross layer boundaries (services → components, engine JS → Rust) without
//! either side holding a signal from the other. Three of these names are also
//! a protocol with the imperative engine, which dispatches them from plain JS
//! and declares them in `public/engine/events.ts`. `tools/check-events.ts`
//! fails CI when the two tables disagree or when a name is spelled as a
//! literal elsewhere — a mismatch is not a compile error on either side, only
//! a dispatch into a window nobody is listening on.
//!
//! The last group is the handoff between a reader and whatever hosts it: the
//! five frames a reader emits and the two it accepts, whose payloads are
//! `reader_core::wire` and whose pairing with these names is
//! `reader_app::wire`. Nothing dispatches them yet — a reader is still a route
//! in the shell's own window, so the shell calls it directly — and that is why
//! the pairing is written down with tests rather than left to the day it is
//! needed: a payload shape is a type the compiler checks, but a name is a
//! string two programs must independently agree on, and no compiler spans it.

use serde::Serialize;

/// AI chunk stream, bridged from the Tauri backend by `services::ai`.
pub const AI_CHUNK_EVENT: &str = "mareader:ai-chunk";
/// Open the gloss card for a mark (carries the `GlossMark` as detail).
pub const GLOSS_OPEN_EVENT: &str = "mareader:gloss-open";
/// Ask for a mark's remove menu (carries the `ContextTarget` as detail).
pub const GLOSS_CONTEXT_EVENT: &str = "mareader:gloss-context";
/// Internal link jump, dispatched by the engine's link layer.
pub const NAVIGATE_EVENT: &str = "mareader:navigate";
/// Page-range selection from the engine's thumbnail/id scanner.
pub const SELECTION_PAGES_EVENT: &str = "mareader:selection-pages";
/// Text-selection detail, dispatched by the engine's text layer.
pub const SELECTION_DETAIL_EVENT: &str = "mareader:selection-detail";
/// One-shot "scroll the sidebar to where the reader is" gesture.
pub const REVEAL_ACTIVE_EVENT: &str = "mareader:reveal-active";
/// Ask the library's title-bar search to take focus. Dispatched by the global
/// Cmd/Ctrl+F when no document is open — the shortcut means "search what you are
/// looking at", and on the library page that is the shelf, not a document. A
/// window event rather than a signal because the bar owns its own input node and
/// the shortcut layer must not know the library page exists.
pub const FOCUS_LIBRARY_SEARCH_EVENT: &str = "mareader:focus-library-search";

/// reader → host: mounted and ready to be told things. The host waits for
/// this before forwarding anything, because a frame sent into a window that
/// has not installed its listeners is dropped rather than queued.
pub const READER_READY_EVENT: &str = "mareader:reader-ready";
/// reader → host: the session's final reading position, on the way out. The
/// same payload the throttled progress frame carries
/// (`reader_core::wire::Progress`) — one fact at two moments.
pub const READER_CLOSED_EVENT: &str = "mareader:reader-closed";
/// reader → host, throttled while reading: where the reading is, so the host
/// can keep the row's resume point without asking the reader for it.
pub const PROGRESS_EVENT: &str = "mareader:progress";
/// reader → host: the paper colour the pane settled on, which the chrome
/// around the document is painted from and the host cannot compute itself.
pub const PANE_PAPER_EVENT: &str = "mareader:pane-paper";
/// reader → host: this pane took focus, and the format it is reading — the
/// shortcuts, menus and settings rows a reader answers to are not the same
/// for a PDF as for a stream of text.
pub const PANE_FOCUS_EVENT: &str = "mareader:pane-focus";
/// host → reader, broadcast: the look, forwarded whole rather than as the
/// field that moved, so every pane converges even if a frame is dropped.
pub const APPEARANCE_BROADCAST_EVENT: &str = "mareader:appearance-broadcast";
/// host → reader: tear down. Payload-less — it is addressed by being
/// dispatched into the one window that is being closed.
pub const DESTROY_EVENT: &str = "mareader:destroy";

/// Dispatch a typed CustomEvent on `window` with `payload` as its detail.
pub fn dispatch_typed_event<T: Serialize>(name: &str, payload: &T) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(detail) = serde_wasm_bindgen::to_value(payload) else {
        return;
    };
    let init = web_sys::CustomEventInit::new();
    init.set_detail(&detail);
    if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(name, &init) {
        let _ = win.dispatch_event(&ev);
    }
}

/// Dispatch a payload-less CustomEvent on `window` — a one-shot gesture with
/// no state to carry (e.g. [`REVEAL_ACTIVE_EVENT`]).
pub fn dispatch_event(name: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    if let Ok(ev) = web_sys::CustomEvent::new(name) {
        let _ = win.dispatch_event(&ev);
    }
}
