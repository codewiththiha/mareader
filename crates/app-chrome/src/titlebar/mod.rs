//! The generic titlebar shell: the hover/pin bar every page renders through,
//! and the shared context it provides to descendants.

pub mod root;

/// The title bar's CSS height in px — Tailwind `h-12` on the bar's row, and
/// the Rust-side single source for every consumer that must agree with it
/// without measuring: the traffic-light centring fallback in this crate, the
/// app's search-reveal dead zone. (The `ResizeObserver` on `#toolbar-row` is
/// the live truth; this is what is assumed before the first observation.)
/// `src-tauri` keeps its own copy — the native shell does not depend on wasm
/// crates — and `tools/check-chrome-contracts.ts` fails CI when the two
/// disagree.
pub const TITLE_BAR_H: f64 = 48.0;
