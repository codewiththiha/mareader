//! One directory per format — what it looks like, not what it is.
//!
//! [`pdf`] rasters sheets and lays them out in a strip. [`reflow`] lays blocks
//! out on an A4 page (or streams them) and asks [`txt`]/[`md`] only for the
//! inside of a block; [`block_render`] is the seam between them. The two
//! [`viewer`](crate::components::viewer) layouts reach these modules through the
//! page host, so no layout, shell or piece of chrome imports a format module
//! directly.
//!
//! This is also the direction of dependency in `crates/`: a format's crate may
//! know its own format and the shared cores, and nothing else. Adding a format
//! means one directory here, one parser crate, and one arm in the host's match.

pub mod block_render;
pub mod md;
pub mod pdf;
pub mod reflow;
pub mod txt;
