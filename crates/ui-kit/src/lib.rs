//! The UI kit shared by the shell and the reader: the generic primitives
//! ([`primitives`]), the window-`CustomEvent` protocol table
//! ([`events`]), and the memory probe ([`memory`]).
//!
//! Three things gathered under one roof for one reason: both wasm builds —
//! the shell and, from Phase 2, the per-document reader instance — render
//! with the same widgets, speak the same event vocabulary, and chart the
//! same heap, and none of the three may know what a document is. The
//! dependency rule is the primitives contract restated at crate scale: this
//! crate never names `state`, `services`, `effects`, or any format
//! crate; consumers depend on it, never the reverse.
//!
//! The chrome crate's own primitives (icon, tooltip, the generic DOM/timer
//! hooks, the layer tokens, floating placement) stay in `app-chrome` —
//! window chrome is not a widget — and the kit wraps them freely.

pub mod events;
pub mod memory;
pub mod primitives;
