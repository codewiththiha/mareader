//! The titlebar family: the app wiring that adapts the generic hover/grab
//! bar shell (`app_title_bar`) to this application, and the centered document
//! title it carries. The popover policy the toolbar menus share lives with the
//! floating primitives it wraps (`ui_kit::floating::menu_popover`).
//!
//! The shell itself (`TitleBar` + `TitleBarCtx`), the native traffic
//! lights and the frameless caption cluster are format-agnostic chrome —
//! they live in the `app-chrome` crate (`app_chrome::titlebar`,
//! `app_chrome::window`).
//!
//! The floating document name is NOT here: it measures the page host under
//! the eyes to decide whether it fits, so it belongs to the reader's viewer
//! (`reader_app::components::viewer::floating_title`) and defers to this
//! family through the `TitleBarCtx` context rather than by living in it.

pub mod app_title_bar;
pub mod document_title;
