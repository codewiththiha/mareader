//! What the app gathers to bookmark a page or highlight text.

use mareader_appearance::Rgb;
use mareader_library::{BookId, MarkId, Pen, Spot};

/// A page to bookmark, or the bookmark already on it.
pub struct Bookmarking<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub spot: Spot,
    /// Where it is, e.g. its chapter, kept with it.
    pub label: String,
    pub existing: Option<MarkId>,
}

/// The reader's highlight, as the app gathers it.
pub struct Highlighted<'a> {
    pub book: Option<BookId>,
    pub name: &'a str,
    pub text: String,
    pub spot: Spot,
    pub pen: Pen,
    /// A colour picked for it; none follows the pen's.
    pub color: Option<Rgb>,
}
