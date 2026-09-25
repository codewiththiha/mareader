//! The state types both runtime surfaces are built from, the UI chrome
//! slice, the shared time/heap/DOM-contract utilities, and the serialized
//! boundary contract the Shell and the runtimes speak (`boundary`).
//!
//! Phase 2 split the one `AppState` by lifetime: `state::reader` stays here
//! while the library's live state moved to the `library-runtime` crate —
//! deliberately blind to each other: a library context cannot name a
//! `ReaderState` and vice versa, which is the compile-level kill switch the
//! phase asks for. The runtime-scoped context structs that
//! bundle these types with their own services live in the runtime crates
//! (`reader_runtime::context`, `library_runtime::context`); the shell's
//! narrow state lives in the root package.

pub mod boundary;
pub mod chrome;
pub mod dom_contract;
pub mod memory;
pub mod state;
pub mod tauri_listen;
pub mod time;

pub use chrome::{ChromeState, ReaderSurface};
pub use state::{AppearanceSignal, CoverImage, CoverMap, ReaderState, SidebarMode, Toast, UiState};
