//! Plain text: the format with no syntax to respect.
//!
//! A `.txt` file has no structure beyond its lines, so this crate's whole job
//! is to find the structure a reader still wants — paragraphs — without
//! inventing any. Blank lines split; hard line breaks are KEPT (the renderer
//! preserves them, which is what makes fixed-line prose, ASCII tables and
//! code-ish notes read as authored); a long paragraph is cut on line
//! boundaries so the paginator can pack a page tightly.
//!
//! Deliberately NOT here: anything that sniffs for markup. A plain-text file
//! containing `#` or `**` shows those characters — the reader opened it as
//! text and asked for the bytes. Markdown's half is `md-core`; both share the
//! block shape, pagination and typography through `reflow-core`.
//!
//! Pure computation: `cargo test -p txt-core`.

#![forbid(unsafe_code)]

pub mod parser;
pub mod subdivide;

pub use parser::parse_plain_text;
pub use reflow_core::source::normalize;
pub use subdivide::subdivide_paragraphs;
