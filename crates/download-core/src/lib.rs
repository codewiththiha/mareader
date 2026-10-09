//! Background downloads: mirrors, range resume, cache, pause and cancel.
//!
//! README.md is the guide.

mod flags;
mod host;
mod job;
mod manager;
mod names;
mod partial;
mod progress;
mod transfer;

pub use host::Host;
pub use job::{Job, ProgressHook, Verify};
pub use manager::{Downloads, Receipt};
pub use names::{file_name_from_url, safe_file_name};
pub use partial::discard;
pub use progress::{Outcome, Phase, Progress};
