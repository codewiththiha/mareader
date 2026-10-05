//! The shell's services: the launch parser (the web test hook), the
//! persistence writes the boundary funnels, the OS-open plumbing that
//! forwards into the live runtime, and the OS import drop that hands files
//! to the library.

mod import_drop;
mod launch;
mod persistence;

pub use import_drop::install_import_drop;
pub use launch::{install_os_open_handling, launch_from_url};
pub use persistence::{apply_read_point, resolve_launch, save_cover, save_gloss, save_settings};
