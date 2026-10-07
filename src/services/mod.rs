//! The shell's services: launch parsing, persistence, OS open and
//! import drop.

mod import_drop;
mod launch;
mod persistence;

pub use import_drop::install_import_drop;
pub use launch::{install_os_open_handling, launch_from_url};
pub use persistence::{apply_read_point, resolve_launch, save_cover, save_gloss, save_settings};
