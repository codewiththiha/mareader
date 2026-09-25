//! The state slices, split by lifetime: the reader's reactive tree, the
//! library's reactive tree, and the UI chrome. No module here can name a
//! slice from another lifetime's context — that impossibility is the point
//! of the Phase 2 split.

pub mod covers;
pub mod reader;
pub mod ui;

pub use covers::{CoverImage, CoverMap};
pub use reader::{ReaderState, TextureSignal};
pub use ui::{AppearanceSignal, SidebarMode, Toast, UiState};
