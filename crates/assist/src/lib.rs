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
mod dispatch;
mod keys;
mod organize;
mod page;
mod prompt;
mod retry;
mod running;
mod service;
mod skill;
mod slots;
mod study_judge;
mod study_mode;
mod task;

pub use apple::Apple;
pub use backend::{Backend, BoxFuture, Nobody, Partial, run};
pub use board::{AppleState, Board, Job, KeyRow, KeyState, Load, Reply, Tried, lanes};
pub use compat::Compat;
pub use dispatch::JobInfo;
pub use keys::KEYCHAIN_SERVICE;
pub use organize::{
    Checkpoint, Existing, Hooks, Loose, NewShelf, Place, Plan, Progress, Request, Spot, Stage,
    organize, organize_from,
};
pub use page::{PAGE_CHARS, PageAsk, PageJob, Rewrite, rewrite};
pub use prompt::{Prompt, quote};
pub use retry::{AHEAD, WAITED, WAITS};
pub use running::{Event, Running, Waker};
pub use service::{Assist, Organized, Organizing, Rewriting, Rewrote};
pub use skill::{SKILL_CHARS, SkillTask, rule};
pub use slots::{Asker, Queued, Slots, Ticket};
pub use study_judge::StudyJudge;
pub use study_mode::{MAX_WORDS, StudyCards};
pub use task::{Explain, Task, word_info_schema};
