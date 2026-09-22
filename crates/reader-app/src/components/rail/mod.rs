//! The rail family: the shared aside container (`container`), the two mount
//! points that own its tree position (`push` docked into the page's flex
//! row, `overlay` floating above it), the header, the book-identity row,
//! the bottom panel switcher, and the panel hosts (`panels`).
//!
//! The rail is the reader's, not the window's: every panel it hosts — the
//! outline, the thumbnails — is a view of the open document, and the layout
//! facts it paints by come from the `ShellController` it is handed. What the
//! shell owns is the rulebook (`app_chrome::controller`) and the bars
//! (`components/shell/titlebar`); what is left here is the reading furniture.

pub mod container;
pub mod document_info;
pub mod header;
pub mod overlay;
pub mod panels;
pub mod push;
pub mod switcher;
