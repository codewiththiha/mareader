//! The production pane: what the reader host creates for each document.
//!
//! - [`base`] — the state/context/surface slice every pane implementation
//!   starts from.
//! - [`document`] — the universal document pane, the [`PaneRuntime`]
//!   implementation (one document session per pane: PDF, Markdown or plain
//!   text through the reader's one pipeline), run one per pane realm.
//! - [`handle`] — the pane's Copy handle, carried in its
//!   [`crate::context::ReaderContext`]: identity, lifecycle gate, document
//!   session, document generation, resource registry.
//! - [`session`] — what a pane's document is owned by: `FormatSession`
//!   (`PdfSession` / `MdSession` / `TxtSession`), one fresh session per
//!   opened document.
//! - [`engine`] — `PdfPane`, the pane's guarded view of its own
//!   `PdfSession`: every PDF engine call the pane makes goes through it.
//! - [`dom`] — the pane's root element and host-given box: every lookup a
//!   pane makes for its own elements runs inside that root.
//! - `view` — the effects the pane installs and the content it renders in
//!   the host's workspace slot.
//!
//! [`PaneRuntime`]: crate::host::contract::PaneRuntime

#[cfg(test)]
mod cascade_tests;
pub(crate) mod base;
pub mod document;
pub mod dom;
#[cfg(feature = "pdf")]
pub mod engine;
pub mod handle;
pub mod origin;
pub mod session;
mod view;
