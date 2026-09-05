//! Markdown: the syntax a reflowable document can carry.
//!
//! `reflow-core` measures blocks; this crate decides what they are. Three
//! things a Markdown file has and a plain-text file does not:
//!
//! * a top-level **construct** per block (heading, fence, list, table, quote,
//!   rule, prose) — [`ast`], which is also what may and may not be split for a
//!   tighter page;
//! * an **outline** — [`outline`] lifts the `#` headings into the reader's
//!   chapter tree, so a Markdown document gets the sidebar panel a PDF's
//!   `/Outlines` dictionary gets, with the page numbers following the live
//!   pagination instead of being baked into the file;
//! * **front matter** — [`metadata`] reads a leading `---` block for a title
//!   and an author before falling back to the first heading.
//!
//! Deliberately NOT here: rendering. The blocks hold their Markdown source and
//! the interface hands each one to a CommonMark renderer; parsing them into
//! HTML here would mean a second Markdown implementation, and the paginator
//! only ever needs the *boundaries* and the *kind*, both of which it can settle
//! with two rules (blank lines, and fences).
//!
//! Pure computation: unit-testable on the host via `cargo test -p md-core`.

#![forbid(unsafe_code)]

pub mod ast;
pub mod metadata;
pub mod outline;
pub mod parser;

pub use ast::{MarkdownConstruct, classify, is_prose_line};
pub use metadata::{document_author, document_title, front_matter};
pub use outline::{MarkdownHeading, extract_headings, headings_of_blocks, headings_to_nodes};
pub use parser::{parse_markdown, subdivide_prose};
