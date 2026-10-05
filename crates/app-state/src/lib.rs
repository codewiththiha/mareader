//! The UI chrome slice the runtime surfaces share, plus the heap/DOM-contract
//! utilities both sides name. The serialized Shell ⇄ runtime boundary, the
//! cover-store types and the clock moved to the `runtime-contract` crate —
//! the one crate cheap enough for every runtime to import, which is exactly
//! why the boundary lives there and not here.
//!
//! Phase 2 split the one `AppState` by lifetime: the reader's state tree
//! lives beside its runtime in the `reader-runtime` crate, the library's
//! live state in the `library-runtime` crate — deliberately blind to each
//! other: a library context cannot name reader state and vice versa, which is
//! the compile-level kill switch the phase asks for. What stays here is only
//! what both lifetimes legitimately share (chrome state, UI signals) and
//! nothing that knows a document engine exists.

pub mod chrome;
pub mod dom_contract;
pub mod memory;
pub mod state;
pub mod tauri_listen;

pub use chrome::{ChromeState, ReaderSurface};
pub use state::{AppearanceSignal, Motion, SidebarMode, Toast, UiState};
