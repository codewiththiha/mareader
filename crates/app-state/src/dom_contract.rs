//! The DOM vocabulary the app shares with the imperative engine.

/// The attribute every reader page host carries, naming its format family.
pub const HOST_ATTR: &str = "data-reader-host";

/// The [`HOST_ATTR`] value a reflowable page or stream block carries.
pub const HOST_REFLOW: &str = "reflow";

/// The [`HOST_ATTR`] value a PDF page carries.
pub const HOST_PDF: &str = "pdf";

/// On a rendered block: which block of the document it is, in document order.
pub const BLOCK_INDEX_ATTR: &str = "data-block-index";

/// On a mounted row that is showing the motion band's placeholder instead of
/// its content. Its box is the layout's own estimate, so measuring it would
/// feed that estimate back to itself as if the reader had typed it.
pub const PLACEHOLDER_ATTR: &str = "data-virtual-placeholder";

/// The class of the still-bitmap overlay a zoom stretches while re-rendering.
pub const PAGE_SNAPSHOT_CLASS: &str = "page-snapshot";

/// The class of the text layer inside a PDF host.
pub const TEXT_LAYER_CLASS: &str = "textLayer";
