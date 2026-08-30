//! Windowing math for virtualized scrolling lists of variably-sized items.
//!
//! At the core sits [`Strip`], a prefix-sum layout engine for one scrolling
//! column of items separated by a fixed gap. Given the size of each item, it
//! answers the four questions a virtualized surface asks every frame:
//!
//! - [`Strip::offset`] — where does item `i` start?
//! - [`Strip::total`] — how large is the whole content extent?
//! - [`Strip::window`] — which items should stay mounted right now?
//! - [`Strip::dominant`] — which item is the reader actually looking at?
//!
//! The crate is pure arithmetic: no DOM, no framework, `no_std`-compatible (`std` enabled by default).
//! Everything is `f64` in whatever unit your app already uses.
//!
//! # Layout layer
//!
//! Above [`Strip`], the crate exposes one shared geometry contract for higher
//! level virtualized surfaces:
//!
//! - [`Layout`] — common queries for item count, offsets, windowing, and
//!   dominant-item selection;
//! - [`ListLayout`] — a variably-sized list backed by [`Strip`];
//! - [`GridLayout`] — a uniform multi-column grid that windows by row while
//!   still answering per-item offsets.
//!
//! This lets a framework adapter hold one layout handle while the app chooses
//! whether the surface is a list or a grid.
//!
//! # Anchoring
//!
//! The [`anchor`] helpers keep the reader's place stable when geometry changes:
//!
//! - [`correct`] adjusts scroll after one measured item changes size;
//! - [`pin_at`] records the content point under a viewport anchor;
//! - [`rescale_anchor`] reapplies that anchor after a uniform rescale.
//!
//! The math stays pure, so adapters can use it from browser, desktop, or test
//! code without any runtime coupling.
//!
//! # Performance
//!
//! The obvious implementation walks the size array to find an item's offset,
//! which is `O(n)` per query and `O(n²)` for a list that positions every
//! mounted item each frame. [`Strip`] stores a prefix-sum table instead,
//! making [`Strip::offset`] `O(1)` and every positional query an `O(log n)`
//! binary search. Building the table is `O(n)`, done once when the sizes
//! change.
//!
//! Internally the prefix-sum is held as `i64` in sub-pixel units (factor
//! [`SUBPIXEL_FACTOR`], 1/65536 px). This gives three wins over the equivalent
//! `Vec<f64>`:
//!
//! - `partition_point` runs over integers, which branch-predict better and
//!   avoid NaN edge cases;
//! - sums cannot drift over long lists;
//! - the storage footprint is identical (`8 * (n + 1)` bytes).
//!
//! For typical continuous scrolling the index is the same as the last frame's,
//! or one step away. [`Strip::index_at_hinted`] takes a `&mut usize` hint and
//! checks the neighbour first, falling back to a galloping search for big
//! jumps (scrollbar drag). That makes smooth scrolling amortized `O(1)`.
//!
//! When items change size at runtime, [`Strip::set_size`] re-runs the suffix of
//! the prefix-sum in `O(n)` time. A surface that resizes items far more often
//! than this one does can supply its own tree by implementing
//! [`StripBackend`]; the windowing above is written against the trait, not
//! against `Strip`.
//!
//! # Example
//!
//! ```
//! use virtual_list::{Budget, Strip};
//!
//! let strip = Strip::new([100.0, 200.0, 100.0], 24.0);
//! assert_eq!(strip.offset(0), 0.0);
//! assert_eq!(strip.offset(1), 124.0);
//! assert_eq!(strip.offset(2), 348.0);
//! assert_eq!(strip.total(), 448.0);
//!
//! let win = strip.visible(0.0, 150.0).unwrap();
//! assert_eq!((win.first, win.last), (0, 1));
//!
//! let _budget = Budget::screenfuls(0.5, 5);
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod anchor;
pub mod backend;
mod layout;
mod units;
mod window;

pub use anchor::{AnchorPolicy, correct, pin_at, rescale_anchor};
pub use backend::{Strip, StripBackend, UniformStrip};
pub use layout::{GridColumns, GridLayout, GridSpec, Layout, LayoutKind, ListLayout};
pub use units::{SUBPIXEL_BITS, SUBPIXEL_FACTOR, from_sub, to_sub};
pub use window::{Align, Budget, Overscan, Viewport, Window};
