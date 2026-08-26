//! The reader sidebar: the composition shell plus its parts — the shell
//! header, the book-identity row, the panel switcher, and the host
//! wrappers around the outline and thumbnail panels.

pub mod document_info;
pub mod header;
pub mod outline_panel;
pub mod outline_view;
pub mod shell;
pub mod switcher;
pub mod thumbnails;
pub mod thumbnails_view;

pub(crate) use shell::Sidebar;
