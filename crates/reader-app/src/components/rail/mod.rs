//! The rail family: the shared aside container (`container`), the two mount
//! points that own its tree position (`push` docked into the page's flex
//! row, `overlay` floating above it), the header, the book-identity row,
//! the bottom panel switcher, and the panel hosts (`panels`).

pub mod container;
pub mod document_info;
pub mod header;
pub mod overlay;
pub mod panels;
pub mod push;
pub mod switcher;
