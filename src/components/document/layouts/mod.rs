//! The four view layouts. Each is thin and shaped identically: it renders its
//! [`PageCanvas`](crate::components::document::PageCanvas) arrangement inside
//! the right shell, with zero scroll or zoom logic of its own.

pub mod scroll_horizontal;
pub mod scroll_vertical;
pub mod single;
pub mod spread;
