//! The runtime contract: the ONLY crate every side of a runtime edge imports.
//!
//! What lives here is boundary-safe data — the launch/read-point/status
//! payloads the Shell and the runtimes exchange (`boundary`), the cover-store
//! types those commands carry (`covers`), the frame wire those exchanges move
//! over once runtimes live in iframes (`protocol`), and the app's one clock
//! (`time`).
//! What deliberately does NOT live here is anything that would let one
//! runtime reach the other by importing a shared helper: no reader state, no
//! library state, no PDF engine or PDF domain types, no virtualizer, no
//! Leptos UI. A contract both runtimes can afford is a contract that cannot
//! smuggle one runtime into the other's dependency graph.

pub mod boundary;
pub mod covers;
pub mod protocol;
pub mod time;

pub use boundary::{DocStatusReport, LaunchDocument, ReadPoint, ShellApi};
pub use covers::{CoverImage, CoverMap};
