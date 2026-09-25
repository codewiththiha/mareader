//! The shell's services: the launch parser (the web test hook), the
//! persistence writes the boundary funnels, and the OS-open plumbing that
//! forwards into the live runtime.

mod launch;
mod persistence;

pub use launch::{install_os_open_handling, launch_from_url};
pub use persistence::{apply_read_point, resolve_launch, save_cover, save_library, save_settings};
