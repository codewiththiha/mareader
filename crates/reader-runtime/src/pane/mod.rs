//! The production pane: what the reader host creates for each document.
//!
//! - [`document`] — the universal document pane, the [`PaneRuntime`]
//!   implementation (one document session per pane: PDF, Markdown or plain
//!   text through the reader's one pipeline), and the [`document::factory`]
//!   the session's composition root injects into the host.
//! - [`handle`] — the pane's Copy handle, carried in its
//!   [`crate::context::ReaderContext`]: identity, lifecycle gate, resource
//!   registry.
//! - [`engine`] — the pane's guarded handle onto the engine's document
//!   session.
//! - `view` — the effects the pane installs and the content it renders in
//!   the host's workspace slot.
//!
//! [`PaneRuntime`]: crate::host::contract::PaneRuntime

pub mod document;
pub mod engine;
pub mod handle;
mod view;
