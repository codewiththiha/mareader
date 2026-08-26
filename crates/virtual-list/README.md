# virtual-list

Pure geometry for virtualized scrolling lists, responsive grids, and scroll anchoring.

At the core sits `Strip`, a prefix-sum layout engine for one scrolling column of variably-sized items separated by a fixed gap. Given item sizes, it answers the four questions a virtualized surface asks every frame:

- **Where does item `i` start?** — `offset`
- **How large is the whole content extent?** — `total`
- **Which items should stay mounted right now?** — `window`
- **Which item is the reader actually looking at?** — `dominant`

It is pure arithmetic: no DOM, no framework, `no_std`-compatible (`std` enabled by default).

## v0.2 layout layer

Above `Strip`, the crate now exposes one shared geometry contract for higher-level virtualized surfaces:

- `Layout` — common queries for item count, offsets, windowing, and dominant-item selection
- `ListLayout` — a variably-sized list backed by `Strip`
- `GridLayout` — a uniform multi-column grid that windows by row while still answering per-item offsets

That lets framework adapters and apps share one geometry API even when a surface can switch between list and grid shapes.

## Anchoring helpers

The `anchor` module keeps the reader's place stable when geometry changes:

- `correct` adjusts scroll after one measured item changes size
- `pin_at` records the content point under a viewport anchor
- `rescale_anchor` reapplies that anchor after a uniform rescale

The crate stays pure, so browser adapters, desktop apps, and tests can all use the same math.

## Why this exists

The naive implementation walks the size array to find an item's offset. That is `O(n)` per query, and a list that positions every mounted item every frame turns it into `O(n²)`.

`Strip` keeps a prefix-sum table instead, so `offset` is `O(1)` and positional queries are `O(log n)` binary searches. Building the table is `O(n)` once per size change.

For continuous scrolling, the answer is usually the same index as last frame or one step away. `index_at_hinted(pos, &mut hint)` checks the neighbour first, then falls back to a galloping search for big jumps like scrollbar drags. That makes smooth scrolling amortized `O(1)`.

## Overscan and ceilings

`Budget` splits mount policy into two orthogonal knobs:

- `overscan`: extra distance or rows to keep warm around the viewport
- `max_items`: a hard ceiling that trims only non-visible items

You can express overscan as:

- `Budget::screenfuls(...)` for zoom-invariant read-ahead
- `Budget::items(...)` for fixed row buffers in cheap grids
- `Overscan::Px(...)` for fixed pixel padding

Two invariants hold for any budget:

1. Every partly-visible item is always mounted.
2. Trimming evicts the item furthest from the viewport first, preferring to keep the one below in reading direction.

## Backends

Three storage backends are available:

| Backend        | Lookup   | Update    | Use when |
| -------------- | -------- | --------- | -------- |
| `Strip`        | `O(1)`   | `O(n)`    | Static or rarely-resized lists |
| `FenwickStrip` | `O(log n)` | `O(log n)` | Highly dynamic lists (`advanced-trees`) |
| `ChunkedStrip` | `O(1)`   | sublinear | Large lists needing fast lookups plus frequent updates (`advanced-trees`) |

## Example

```rust
use virtual_list::{Budget, Strip};

let strip = Strip::uniform(500, 800.0, 24.0);
assert_eq!(strip.offset(0), 0.0);
assert_eq!(strip.offset(1), 824.0);

let budget = Budget::screenfuls(1.0, 7);
let win = strip.window(10_000.0, 900.0, budget).unwrap();
assert!(win.contains(strip.index_at(10_000.0)));

let mut hint = 0usize;
for top in (0..10_000).map(|i| i as f64 * 7.3) {
    let _ = strip.index_at_hinted(top, &mut hint);
}
```

## License

MIT
