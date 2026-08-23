//! Windowing math for virtualized scrolling lists of variably-sized items.
//!
//! A [`Strip`] is a column of items laid out one after another with a fixed gap
//! between them. It answers the four questions a virtualized list asks every
//! frame:
//!
//! - [`Strip::offset`] — where does item `i` start?
//! - [`Strip::total`] — how tall is the whole column?
//! - [`Strip::window`] — which items should be mounted right now?
//! - [`Strip::dominant`] — which item is the reader actually looking at?
//!
//! Everything is `f64` in a unit of your choosing (CSS px, points, logical
//! pixels). There is no DOM and no framework here: feed it sizes, get back
//! indices and offsets.
//!
//! # Performance
//!
//! The obvious implementation walks the size array to find an item's offset,
//! which is `O(n)` per query and `O(n²)` for a list that positions every
//! mounted item each frame. [`Strip`] stores a prefix-sum table instead, making
//! [`Strip::offset`] `O(1)` and every positional query an `O(log n)` binary
//! search. Building the table is `O(n)`, done once when the sizes change.
//!
//! Internally the prefix-sum is held as `i64` in sub-pixel units (factor
//! [`SUBPIXEL_FACTOR`], 1/65536 px). This gives three wins over the equivalent
//! `Vec<f64>`:
//!
//! - `partition_point` runs over integers, which branch-predict better and
//!   avoid NaN edge cases;
//! - sums cannot drift over long lists — `i64` is exact up to ~280 billion
//!   sub-pixels, i.e. ~4.2 million CSS pixels of total extent;
//! - the storage footprint is identical (`8 * (n + 1)` bytes) and there is no
//!   conversion cost beyond the API boundary.
//!
//! For typical continuous scrolling the index is the same as the last frame's,
//! or one step away. [`Strip::index_at_hinted`] takes a `&mut usize` hint and
//! checks the neighbour first, falling back to a galloping search for big
//! jumps (scrollbar drag). That makes smooth scrolling **amortized `O(1)`**.
//!
//! When items change size at runtime (an image finishes loading, an accordion
//! expands, an estimated size is replaced by a measured one) [`Strip::set_size`]
//! re-runs the suffix of the prefix-sum in `O(n)` time. For lists that mutate
//! sizes faster than that can pay, enable the `advanced-trees` feature for
//! `FenwickStrip` (BIT, `O(log n)` update/lookup) and `ChunkedStrip`
//! (sqrt-decomposition, `O(1)` lookup / `O(sqrt n)` update). pdf-reader only
//! uses [`Strip`].
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
//! // What is on screen in a 150-tall viewport parked at the top?
//! let win = strip.visible(0.0, 150.0).unwrap();
//! assert_eq!((win.first, win.last), (0, 1));
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

mod strip;
mod units;

#[cfg(feature = "advanced-trees")]
mod chunked;
#[cfg(feature = "advanced-trees")]
mod fenwick;

pub use strip::{Budget, Strip, Window};
#[cfg(feature = "advanced-trees")]
pub use chunked::ChunkedStrip;
#[cfg(feature = "advanced-trees")]
pub use fenwick::FenwickStrip;
pub use units::{from_sub, to_sub, SUBPIXEL_BITS, SUBPIXEL_FACTOR};
