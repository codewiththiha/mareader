//! The single owner of the relationship between pages, scroll position, and
//! scale.
//!
//! Before this module existed, views, zoom, fit, and navigation each reached
//! into the virtualizers and the DOM directly, and zoom duplicating the
//! axis-branching relayout across two code paths was the root of the races.
//! Everything in the viewer that must move geometry (rescale a strip, jump
//! to a page, report a rendered size) goes through [`engine::ViewerEngine`];
//! everything that decides what the zoom should be goes through
//! [`zoom::ZoomController`]. Nothing outside them should touch a
//! virtualizer's layout or a zoom scale.

pub mod engine;
pub mod zoom;
