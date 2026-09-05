//! App-level state: settings, the reader slice (document/viewer/search),
//! the library, and the UI chrome. Pure domain logic lives in `pdf-core`;
//! the engine bridge in `pdf-engine`; document lifecycle operations in
//! `services`.

pub mod app;
pub mod library;
pub mod reader;

pub use app::{AppearanceSignal, AppState, SidebarMode, Toast};
pub use reader::{NO_DOCUMENT, ReaderState, TextureSignal};
