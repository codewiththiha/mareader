//! The memory probe's import path in the shell.
//!
//! The probe lives in `ui-kit` (`crates/ui-kit/src/memory.rs`): the
//! reader instance charts the same three pools with the same one line, and
//! Phase 2's teardown proof reads both builds' traces against each other.
//! The names are re-exported unchanged, so the shell's call sites keep
//! reading `crate::memory`.

pub use ui_kit::memory::*;
