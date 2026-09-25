//! The shared UI surface: primitives, overlays, the appearance menu and its
//! scrub pipeline, the app title bar and the shell layout controller — the
//! chrome code both runtime surfaces compile (AGENTS.md: shared chrome code =
//! reusable compiled code; chrome STATE belongs to whichever runtime is
//! showing it). The reader-only component tree (viewer, formats, ai, search,
//! settings, sidebar) lives in `reader-runtime`, which also depends on this
//! crate.

pub mod appearance;
pub mod components;
pub mod epoch;
pub mod events;
pub mod theme_paint;
