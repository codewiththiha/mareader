//! The reader's feature surfaces. The session host (the old `ReaderPage`,
//! now the runtime boundary) owns the chrome composition and the effect
//! wiring; the rail is the sidebar's content tree.

pub mod page;
pub mod rail;
pub mod virtualizers;

pub use page::ReaderPage;
