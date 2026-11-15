//! The app without a window: what outlives a frame or reaches disk (L5).
//!
//! Nothing here names a toolkit (`docs/architecture.md` § Layers). What it
//! holds is what outlives a frame or reaches disk; a front end draws it, and
//! changes it only by handing an command to `Session::execute`.
//!
//! Paths here keep their front-end modules (`texts`, `paper`, `read_aloud`),
//! because a move must not rename what it moves.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod appearance_menu;
pub mod command;
pub mod command_spec;
pub mod dict;
pub mod documents;
pub mod event;
mod execute;
pub mod export;
pub mod host;
pub mod library;
pub mod lru;
pub mod memory;
pub mod notes_panel;
pub mod options;
pub mod page_ai;
pub mod page_copy;
pub mod pane;
pub mod pane_texts;
pub mod paper;
pub mod play;
pub mod read_aloud;
pub mod reading;
pub mod reading_place;
pub mod repaint;
pub mod route;
pub mod sample;
pub mod save;
/// Which settings apply to the focused pane's document.
pub mod scope;
pub mod scripts;
pub mod search;
pub mod session;
pub mod settings;
pub mod sounds;
pub mod stable_hash;
pub mod study;
#[cfg(test)]
mod testing;
pub mod texts;
pub mod toast;
pub mod transcript;
pub mod triggers;
pub mod vocab;
pub mod watch;
pub mod word_card;
pub mod word_list;
pub mod word_marks;
