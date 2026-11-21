//! AI for the study tools (L3): tasks, any backend; see `docs/assist.md`.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod apple;
mod backend;
mod board;
mod compat;
mod keys;
mod organize;
mod page;
mod prompt;
mod service;
mod slots;
mod task;

pub use apple::Apple;
pub use backend::{Backend, BoxFuture, Nobody, Partial, run};
pub use board::{AppleState, Board, Job, KeyRow, KeyState, Load, Reply, Tried, lanes};
pub use compat::Compat;
pub use keys::KEYCHAIN_SERVICE;
pub use organize::{
    Checkpoint, Existing, Hooks, Loose, NewShelf, Place, Plan, Progress, Request, Spot, Stage,
    WAITS, organize, organize_from,
};
pub use page::{PAGE_CHARS, PageAsk, PageJob, Rewrite, rewrite};
pub use prompt::{Prompt, quote};
pub use service::{Assist, Organized, Organizing, Rewriting, Rewrote, Waker};
pub use slots::{Kind, Slots, Ticket};
pub use task::{Explain, StudyCards, StudyJudge, Task, word_info_schema};
