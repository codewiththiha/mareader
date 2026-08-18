# virtual-list

Windowing math for virtualized scrolling lists of variably-sized items.

Given the sizes of every item in a scrolling column, this crate answers the four
questions a virtualized list has to answer on every frame:

- **Where does item `i` start?** — `offset`
- **How tall is the whole thing?** — `total`
- **Which items should be mounted right now?** — `window`
- **Which item is the reader actually looking at?** — `dominant**

It is pure arithmetic: no DOM, no framework, no allocator tricks, `no_std` by
default-off. Everything is `f64` in whatever unit you like (CSS px, points,
logical pixels).

## Why not just loop over the sizes?

The naive implementation walks the size array to find an item's offset. That is
`O(n)` per query, and a list that positions every mounted item per frame turns
it into `O(n²)`. At 14 items nobody notices; at 2,000 items it is the reason
scrolling stutters.

`Strip` keeps a prefix-sum table, so `offset` is `O(1)` and every positional
query is an `O(log n)` binary search. Building the table is `O(n)` once per size
change.

## Internals: `i64` sub-pixels, galloping search, and three backends

The prefix-sum table is held as `i64` in **sub-pixel units** (`1 << 16` per
CSS pixel). That gives three wins over a `Vec<f64>`:

- `partition_point` runs over integers — no NaN edge cases, faster
  branch-predicted comparisons.
- Sums cannot drift over long lists. `i64` is exact up to ~4.2e9 CSS pixels
  (~4 200 km).
- The storage footprint is identical (`8 * (n + 1)` bytes).

For **continuous scrolling**, the answer to "which item is at the top of the
viewport?" is almost always the same index or one step away from the previous
frame. `index_at_hinted(pos, &mut hint)` checks the neighbour first, and falls
back to a **galloping search** (probe 1, 2, 4, 8 steps outwards, then binary
search the bracket) for big jumps like scrollbar drags. That makes smooth
scrolling **amortized `O(1)`**.

For **dynamic item sizes** (an image finishes loading, an accordion expands),
the crate ships three backends:

| Backend         | Lookup   | Update    | Use when                                       |
| --------------- | -------- | --------- | ---------------------------------------------- |
| `Strip`         | `O(1)`   | `O(n)`    | Static or rarely-resized lists                  |
| `FenwickStrip`  | `O(log n)` | `O(log n)` | Highly-dynamic (chat, live logs, accordions)   |
| `ChunkedStrip`  | `O(1)`   | `O(sqrt n)` | Large lists needing both fast lookups and dynamic updates |

`FenwickStrip` uses a Binary Indexed Tree with **binary lifting** — no
separate prefix-sum array is consulted on lookup.

`ChunkedStrip` uses square-root decomposition: an immutable per-item prefix-sum
plus a small per-chunk cumulative delta register. The chunk size is picked
automatically as `max(16, sqrt(n))`.

## Advanced features

- **Scroll anchoring** — when an item above the viewport changes size, call
  `scroll_anchor_delta(scroll_top, anchor_index, delta)` to get the corrected
  scroll position so the reader's view doesn't jump.
- **Sticky headers** — `window_with_sticky(scroll_top, viewport, budget,
  &sticky_indices)` accepts a list of sticky item indices; the one with the
  largest index whose start is at or above `scroll_top` is "pinned" to the
  top of the viewport, and items below it scroll underneath. The pinned sticky
  itself is always included in the returned window.
- **Estimated / placeholder heights** — `Strip::with_estimated(count,
  estimated_size, gap)` builds a strip with uniform placeholders; the caller
  then refines each item with `set_size(index, measured_size)` as the real
  dimensions become known.

## Example

```rust
use virtual_list::{Budget, Strip};

// A column of 500 pages, 800.0 tall each, separated by a 24.0 gap.
let strip = Strip::uniform(500, 800.0, 24.0);

assert_eq!(strip.offset(0), 0.0);
assert_eq!(strip.offset(1), 824.0);

// Mount what is on screen, plus one screenful of read-ahead each way,
// but never more than 7 items at once.
let budget = Budget { look_frac: 1.0, max_items: 7 };
let win = strip.window(10_000.0, 900.0, budget).unwrap();
assert!(win.contains(strip.index_at(10_000.0)));

// Continuous scrolling: hint-based galloping search is amortized O(1).
let mut hint = 0usize;
for top in (0..10_000).map(|i| i as f64 * 7.3) {
    let _ = strip.index_at_hinted(top, &mut hint);
}

// An image finished loading on item 12; resize it. The scroll anchor is
// the reader's current page (item 30, scroll_top=24_000).
let mut strip = Strip::uniform(500, 800.0, 24.0);
let delta = strip.set_size(12, 1100.0);
let new_top = strip.scroll_anchor_delta(24_000.0, 30, delta);
// The reader stays anchored on item 30.
```

## Guarantees

Two invariants hold for **any** `Budget`, which is what makes the budget safe to
expose as a user-facing setting:

1. Every item that is even partly visible is always in the window. No budget can
   blank out something the reader is looking at.
2. Trimming to `max_items` evicts the item furthest from the viewport first, and
   prefers to keep the item *below* (reading direction), so the next item the
   reader will reach is the last one dropped.

Both are covered by exhaustive tests, including a fuzz-style sweep over
scroll positions with `max_items: 1`.

## License

MIT
