//! Generic effect hooks: each owns one listener/effect family so components
//! read as wiring + view, and the raw pattern (closure parking, cleanup
//! ordering, JS-reference lifetimes) is written once.
//!
//! Contract: nothing here may know what a PDF reader is. These are the
//! layer-1 primitives the floating/interaction systems compose.

pub mod dom;
pub mod use_content_size;
pub mod use_custom_event;
pub mod use_raf;
pub mod use_resize_observer;
pub mod use_timeout;
pub mod use_viewport;
pub mod use_window_event;
