//! The UI chrome slice the runtimes share: toasts, the sidebar mode, the
//! motion projection, the appearance signal alias. Reader state lives in
//! `reader-runtime`, library state in `library-runtime`, and the
//! boundary/cover types in `runtime-contract` — no module here can name a
//! document engine.

pub mod ui;

pub use ui::{AppearanceSignal, Motion, SidebarMode, Toast, UiState};
