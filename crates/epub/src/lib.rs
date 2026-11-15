//! EPUB books through rbook: chapters read into reflow blocks (L2).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod book;
mod calc;
mod convert;
mod css;
mod decor;
mod fontfile;
mod generated;
mod guard;
mod href;
mod math;
mod media;
mod notes;
mod overlay;
mod query;
pub mod sample;
mod script;
mod splice;
mod subtitles;
mod svg;
mod trigger;
mod xhtml;

pub use book::{
    Book, Destination, Files, Link, MAX_BLOCKS, MAX_MARKUP, MAX_PICTURE_BYTES, MAX_TOC, OpenBook,
    OpenStep, OpenTarget, SpinePlace, TocEntry, cover, describe,
};

#[cfg(not(target_arch = "wasm32"))]
pub use book::{cover_file, describe_file};

pub use convert::{Format, MAX_PICTURE_FILE, MAX_TEXT, Pictures, convert};
pub use css::MAX_SHEET;
pub use guard::{MAX_ENTRIES, MAX_ENTRY, MAX_REFINES, MAX_TOC_DEPTH, MAX_TOTAL};
pub use media::Media;
pub use overlay::{MAX_PHRASES, Narration, Phrase, Run, Skips, Spoken};
pub use script::Scripted;
pub use splice::{Insert, Inserts};
pub use subtitles::{Cue, MAX_TRACKS, Subtitle, Track, Wanted, cues, pick, stamp, track_name};
pub use trigger::{Action, Triggered};
pub use xhtml::{MAX_DEPTH, MAX_LINKS, MAX_PLACES, MAX_SHEETS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("the file isn't a readable EPUB")]
    Invalid,
    #[error("the book has no text to show")]
    Empty,
    #[error("the book is too large to open safely")]
    TooLarge,
}

/// Whether `bytes` look like a zip, as every EPUB is.
pub fn sniff(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04")
}
