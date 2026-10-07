//! Markdown: block constructs, the heading outline, and front matter.

#![forbid(unsafe_code)]

pub mod ast;
pub mod metadata;
pub mod outline;
pub mod parser;

pub use metadata::{document_author, document_title};
pub use outline::{MarkdownHeading, headings_of_blocks, headings_to_nodes};
pub use parser::{parse_markdown, subdivide_prose};
