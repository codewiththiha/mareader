//! The reader-only component tree: the viewing machinery, the format
//! renderers, the AI surfaces, search presentation, the settings modal and
//! the reader's rail family + document titles. The shared chrome this tree
//! builds on (primitives, appearance menu, app title bar, controller) lives
//! in `app-ui`.

pub mod ai;
pub mod formats;
pub mod menus;
pub mod search;
pub mod settings;
pub mod shell;
pub mod viewer;
