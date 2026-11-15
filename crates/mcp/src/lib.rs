//! Mareader Model Context Protocol (MCP) server and live UI control client.
//!
//! Sits at L6 with zero UI toolkit dependencies and zero async runtime.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

pub mod backend;
pub mod client;
pub mod headless;
pub mod image_io;
pub mod jail;
pub mod server;
pub mod tools;
pub mod ui_catalog;
