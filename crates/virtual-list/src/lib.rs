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
//! Pure arithmetic: no DOM, no framework, `no_std`-compatible (`std` enabled
//! by default). Everything is `f64` in whatever unit the app uses.
//!
//! # Layout layer
//!
//! Above [`Strip`], one shared geometry contract for higher-level surfaces:
//! [`Layout`] (common queries for item count, offsets, windowing and
//! dominant-item selection), [`ListLayout`] (a variably-sized list backed by
//! [`Strip`]) and [`GridLayout`] (a uniform multi-column grid that windows by
//! row while still answering per-item offsets). A framework adapter holds one
//! layout handle whichever the surface is.
//!
//! # Anchoring
//!
//! The [`anchor`] helpers keep the reader's place stable when geometry
//! changes: [`correct`] adjusts scroll after one measured item changes size,
//! [`pin_at`] records the content point under a viewport anchor, and
//! [`rescale_anchor`] reapplies that anchor after a uniform rescale.
//!
//! # Performance
//!
//! [`Strip`] stores a prefix-sum table instead of walking the size array:
//! [`Strip::offset`] is `O(1)`, every positional query an `O(log n)` binary
//! search, the table built `O(n)` once when sizes change — where the naive
//! walk is `O(n)` per query and `O(n²)` per frame. The sums are held as `i64`
//! in sub-pixel units (`SUBPIXEL_FACTOR`, 1/65536 px): integer
//! `partition_point` branch-predicts better and avoids NaN edge cases, sums
//! cannot drift over long lists, and the footprint equals a `Vec<f64>`.
//! Smooth scrolling keeps the index at last frame's or one step away, so
//! [`Strip::index_at_hinted`] checks the neighbour first and falls back to a
//! galloping search for big jumps (scrollbar drag) — amortized `O(1)`.
//! Runtime resizes re-run the prefix-sum suffix in `O(n)`; a surface that
//! resizes far more often can supply its own tree via [`StripBackend`] — the
//! windowing is written against the trait, not against `Strip`.
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
pub use backend::{Strip, StripBackend};
pub use layout::{GridColumns, GridLayout, GridSpec, Layout, LayoutKind, ListLayout};
pub use window::{Align, Budget, Overscan, Viewport, Window};
