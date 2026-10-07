//! Plain text: blocks split on blank lines, hard line breaks kept.

#![forbid(unsafe_code)]

pub mod parser;
pub mod subdivide;

pub use parser::parse_plain_text;
pub use subdivide::subdivide_paragraphs;
