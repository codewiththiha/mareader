//! The generic titlebar shell: the hover/pin bar every page renders through,
//! and the shared context it provides to descendants.

pub mod root;

/// The title bar's CSS height in px — Tailwind `h-12` on the bar's row.
///
/// The Rust-side single source for every consumer that must agree with the
/// bar's rendered height without measuring it: the traffic-light centring's
/// fallback in this crate, the app's search-reveal dead zone. (The height the
/// `ResizeObserver` on `#toolbar-row` reports at runtime is the live truth;
/// this is what is assumed before the first observation lands.) `src-tauri`
/// keeps its own copy — the native shell does not depend on wasm crates —
/// and points back here in prose.
///
/// MUST stay in sync with the `h-12` classes the title bar views render.
pub const TITLE_BAR_H: f64 = 48.0;
