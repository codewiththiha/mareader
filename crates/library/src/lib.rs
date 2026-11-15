//! The library as data (L1): books, positions, pure rules; see
//! `docs/library.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod absorb;
mod ai_pages;
mod arrange;
mod book;
mod clock;
pub mod conflict;
pub mod disk;
mod extra;
mod folder;
pub mod gate;
pub mod ground;
mod holding;
mod id;
mod key;
pub mod ledger;
mod legacy;
mod library;
mod link;
mod marks;
mod merge;
pub mod moves;
mod query;
mod records;
mod relink;
mod removal;
pub mod run;
pub mod screen;
mod shelves;
#[cfg_attr(target_arch = "wasm32", path = "store_none.rs")]
pub mod store;
mod tracked;
mod tree;
mod view;
mod words;

pub use absorb::Absorbed;
pub use ai_pages::{AiPage, AiTask, NewAiPage, PageSpan};
pub use arrange::Item;
pub use book::{
    Book, Design, FileStamp, Format, Location, Origin, Position, SpinePosition, State,
    display_title,
};
pub use clock::{Clock, Field, Stamp};
pub use conflict::{Arrival, Entry, Level, Placement};
pub use extra::Extra;
pub use folder::{
    Folder, Found, MIN_SIZE_MAX, MIN_SIZE_STEP, Mode, Options, Rungs, Tombstone, dir_of_rung,
    parent_key,
};
pub use holding::Holding;
pub use id::{AiPageId, BookId, ContentId, DeviceId, FolderId, LinkId, MarkId, ShelfId};
pub use ledger::{Held, Recovered};
pub use library::{Dirty, Library, Measure, Opened};
pub use link::{Link, LinkTo};
pub use marks::{Gloss, Mark, MarkKind, NewMark, Pen, Spot, Style};
pub use merge::Merge;
pub use moves::{Departure, Move, MoveAsk, Moved};
pub use query::{Hit, Query, SUGGEST_LIMIT, Suggestion, Target};
pub use records::{Changes, Kind, Row};
pub use relink::RelinkError;
pub use removal::{Data, Receipt, Removing};
pub use run::{Asked, Copied, Fold, Landed, MergeAsk, Plan, Planned};
pub use shelves::{Membership, NAME_MAX, Shelf, ShelfError, ShelfKind, ShelfRemoved, ShelfWatch};
pub use tree::Duplicated;
pub use view::{COLUMNS_MAX, COLUMNS_MIN, CoverFit, Home, Layout, SortKey, View, natural};
pub use words::{free_name, human_age, human_size, plural, same_name};

#[cfg(test)]
mod absorb_tests;
#[cfg(test)]
mod arrange_tests;
#[cfg(test)]
mod conflict_tests;
#[cfg(test)]
mod folder_tests;
#[cfg(test)]
mod gate_tests;
#[cfg(test)]
mod ground_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod legacy_tests;
#[cfg(test)]
mod marks_tests;
#[cfg(test)]
mod moves_tests;
#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod relink_tests;
#[cfg(test)]
mod removal_tests;
#[cfg(test)]
mod run_tests;
#[cfg(test)]
mod shelves_tests;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
mod stored_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod words_tests;
