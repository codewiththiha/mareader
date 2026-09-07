//! Markdown: the syntax a reflowable document can carry.
//!
//! `reflow-core` measures blocks; this crate decides what they are. Three
//! things a Markdown file has and a plain-text file does not:
//!
//! * a top-level **construct** per block (heading, fence, list, table, quote,
//!   rule, prose) — [`ast`], which is also what may and may not be split for
//!   a tighter page;
//! * an **outline** — [`outline`] lifts the `#` headings into the reader's
//!   chapter tree, with page numbers following the live pagination instead of
//!   being baked into the file;
//! * **front matter** — [`metadata`] reads a leading `---` block for a title
//!   and author before falling back to the first heading.
//!
//! Deliberately NOT here: rendering. Blocks hold their Markdown source and
//! the interface hands each to a CommonMark renderer; parsing to HTML here
//! would be a second Markdown implementation, and the paginator only needs
//! the *boundaries* and the *kind* — settled by two rules (blank lines,
//! fences).
//!
//! Pure computation: `cargo test -p md-core`.

#![forbid(unsafe_code)]

pub mod ast;
pub mod metadata;
pub mod outline;
pub mod parser;

pub use metadata::{document_author, document_title};
pub use outline::{MarkdownHeading, headings_of_blocks, headings_to_nodes};
pub use parser::{parse_markdown, subdivide_prose};
