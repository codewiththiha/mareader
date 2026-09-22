//! The reader's compositions: the parts that are assembled out of more than
//! one component group.
//!
//!   * `root` — [`root::ReaderRoot`], everything below the title bar: the
//!     rail's two mount points, the viewer slot, the floating surfaces, and
//!     the effect installations that make them agree.
//!   * `rail` — the rail's four slots, written once and mounted from either
//!     mount point.
//!   * `virtualizers` — the two virtualizer handles the strips scroll with,
//!     built once and shared by the viewer components and the effects.

pub mod rail;
pub mod root;
pub mod virtualizers;

pub use root::ReaderRoot;
pub(crate) use virtualizers::use_reader_virtualizers;
