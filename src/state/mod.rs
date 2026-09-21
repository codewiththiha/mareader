//! The shell's state: settings, the library, and the UI chrome. The
//! reader slice (document/viewer/search/AI) lives in the reader crate
//! (`reader_app::state`) and is re-exported below at its old path. Pure
//! domain logic lives in `pdf-core`; the engine bridge in `pdf-engine`;
//! document lifecycle operations in `services`.

pub mod app;
pub mod library;

pub use app::{AppearanceSignal, AppState, Toast};
pub use reader_app::state::{GlossSave, ReaderState, TextureSignal};
pub use reader_core::ui::SidebarMode;
