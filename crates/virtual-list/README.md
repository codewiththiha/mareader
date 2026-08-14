# virtual-list

Windowing math for virtualized scrolling lists of variably-sized items.

Given the sizes of every item in a scrolling column, this crate answers the four
questions a virtualized list has to answer on every frame:

- **Where does item `i` start?** — `offset`
- **How tall is the whole thing?** — `total`
- **Which items should be mounted right now?** — `window`
- **Which item is the reader actually looking at?** — `dominant`

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
