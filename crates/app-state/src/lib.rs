//! The UI chrome slice the runtime surfaces share, plus its heap utilities.

pub mod chrome;
pub mod dom_contract;
pub mod memory;
pub mod state;
pub mod tauri_listen;

pub use chrome::{ChromeState, ReaderSurface};
pub use state::{AppearanceSignal, Motion, SidebarMode, Toast, UiState};
