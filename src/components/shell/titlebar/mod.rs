//! The titlebar family: the app wiring that adapts the generic hover/grab
//! bar shell (`app_title_bar`) to this application, the document titles,
//! and the popover policy the toolbar menus share.
//!
//! The shell itself (`TitleBar` + `TitleBarCtx`), the native traffic
//! lights and the frameless caption cluster are format-agnostic chrome —
//! they live in the `app-chrome` crate (`app_chrome::titlebar`,
//! `app_chrome::window`).

pub mod app_title_bar;
pub mod constants;
pub mod document_title;
pub mod floating_document_title;
pub mod toolbar_popover;
