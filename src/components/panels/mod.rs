//! The reader sidebar: the composition shell plus its parts — the chrome
//! header, the book-identity row, the panel switcher, and the host
//! wrappers around the outline and thumbnail panels.

pub mod book_info;
pub mod outline;
pub mod outline_host;
pub mod panel_switcher;
pub mod sidebar_header;
pub mod sidebar_shell;
pub mod thumbnail_host;
pub mod thumbnails;

pub(crate) use sidebar_shell::Sidebar;
