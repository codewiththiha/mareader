//! Plain text: the format with no syntax to respect.
//!
//! A `.txt` file has no structure beyond its lines, so this crate's whole job
//! is to find the structure a reader still wants — paragraphs — without
//! inventing any. Blank lines split, hard line breaks are KEPT (the renderer
//! preserves them, which is what makes fixed-line prose, ASCII tables and
//! code-ish notes read as authored), and a long paragraph is cut on line
//! boundaries so the paginator can pack a page tightly.
//!
//! Deliberately NOT here: anything that sniffs for markup. A plain-text file
//! that happens to contain `#` or `**` shows those characters, because a
//! reader who opened it as text asked for the bytes. Markdown's half of the
//! pipeline is `md-core`, and both share the block shape, the pagination and
//! the typography through `reflow-core`.
//!
//! Pure computation: unit-testable on the host via `cargo test -p txt-core`.

#![forbid(unsafe_code)]

pub mod parser;
pub mod subdivide;

pub use parser::parse_plain_text;
pub use reflow_core::source::normalize;
pub use subdivide::subdivide_paragraphs;
