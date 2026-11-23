//! What an AI task does when an answer fails.
//!
//! Plain data, like `ai_job.rs`, so it sits below the crate that runs the
//! work and the one that commands it.

use std::time::Duration;

/// How often a task asks again, and how long it waits first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retry {
    /// Times an unreadable answer is asked again.
    pub unreadable: u8,
    /// Pauses before asking again when a server is briefly away.
    pub waits: &'static [Duration],
}
