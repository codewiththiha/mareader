//! App-level state: settings, the reader slice (document/viewer/search),
//! the library, and the UI chrome. Pure domain logic lives in `pdf-core`;
//! the engine bridge in `pdf-engine`; document lifecycle operations in
//! `services`.

pub mod app;
pub mod library;
pub mod reader;
pub mod ui;

pub use app::{AppState, Toast};
pub use reader::{NO_DOCUMENT, ReaderState, TextureSignal};
pub use ui::SidebarMode;
