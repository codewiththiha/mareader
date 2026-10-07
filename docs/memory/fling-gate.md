# The fling gate

How the PDF strip decides when a page may start a full-resolution raster
while the reader scrolls, why that decision is a memory device, and the
rules any change to it must keep.

## What the gate protects

The render effect in
`crates/reader-runtime/src/components/formats/pdf/canvas.rs` starts a page
raster. While the strip moves (`settled == Some(false)`), an unpainted page
that is not in view does not start one; it stays blank. There is no
placeholder: no thumbnail underlay, no cached low-resolution copy, no
stretched bitmap.

Two facts of browser memory make the gate necessary:

1. A raster sets the page canvas to full page pixels (10–25 MB of backing
   store at reading zoom).
2. The webview's resource cache grows to the peak of surfaces it has seen
   and does not hand that peak back at idle. Churn — large surfaces created
   and discarded every few frames — drives the peak up even though no
   single moment holds much.

Rasterising every page a fling flies past is exactly that churn.

## Speed-aware visibility

`in_view_signal` (`crates/reader-runtime/src/components/formats/pdf/strip.rs`)
is the virtualizer's own answer — an item is visible while its state is
`Active` — and the render effect reads it tracked, beside `settled`:

| Situation | Behaviour |
| --- | --- |
| Page in the band at reading speed | Visible at once; renders while the strip moves |
| Page in the band while the strip is engaged | Visible, because the band leads the reader: it opens on a smoothed velocity and widens with it |
| Page the fling only sweeps past | Not in the band, so it renders at the scroll settle |
| Zoom in flight | The effect's `anim` branch runs before the gate |
| Page modes | No `in_view` signal; pages render immediately |

The band is a signal, not a deadline: it moves when the motion model moves,
so visibility never waits on another scroll event arriving. Its shape is
`MotionConfig` (`crates/virtual-list/src/motion.rs`) — a 32 ms smoothing
constant, an entry floor of 1400 px/s (~2 screens/s on a 700 px window), a
0.4 hysteresis so a settled reader lets go, and 0.5–2.0 screens of lead with
0.25 of a screen warm behind. `Pipeline::enter_px_s` raises that floor to
what the content lane can actually fill, so a slow pipeline is not handed a
band it cannot keep up with.

## Never stuck at a stale scale

A render that lands after the committed scale moved on
(`Completion::Stale`) leaves a bitmap at the wrong scale, stretched to the
display size. The page host forces its render effect to run again
immediately (a component-owned `Trigger`), so the crisp render follows
without waiting for an unrelated dependency change.

## Rules for changes

- Keep the gate. A visible page renders while moving; a page a fling only
  sweeps past does not.
- Never add a path where a page's correctness depends on a later event that
  may not come. Every deferral needs a guaranteed wake (settle, timer,
  trigger).
- Do not reintroduce placeholder pixels. Blank until the full-resolution
  render lands is the contract.
- Measure with a scroll-heavy workload. The engine smoke suite has no DOM,
  the lifecycle suite asserts behaviour, and the memory replay scrolls each
  pane once, so none of them sees churn on its own.

## Constants

| Constant | Value | Owner |
| --- | --- | --- |
| Scroll settle delay | 150 ms | virtualizer `scroll_end_delay_ms` |
| `enter_floor_px_s` / `hysteresis` | 1400 px/s / 0.4 | `virtual-list/src/motion.rs` |
| `min_lead_screens` / `max_lead_screens` / `trail_screens` | 0.5 / 2.0 / 0.25 | `virtual-list/src/motion.rs` |
| `tau_ms` / `flip_ratio` | 32 ms / 0.35 | `virtual-list/src/motion.rs` |
| `CLEANUP_EVERY` | 5 renders | `engine/state.ts` |
| `SWEEP_IDLE_MS` | 30 s | `engine/state.ts` |
| `RAW_IDLE_MS` | 2 s | `engine/state.ts` |
| `MAX_ZOMBIES` / `ZOOM_GRACE_MS` | 12 / 300 ms | `reader-runtime/src/zoom/config.rs` |
| `PAGE_RENDER_LIMIT` / `REALM_PAGE_LIMIT` | 2 / 2 | `engine/state.ts` |

## History

A first version exempted visible pages the moment they crossed into the
band. During a fling every page crosses, so each started a raster that was
discarded frames later; the webview latched the higher footprint. A 120 ms
dwell fixed the churn but left pages blurry on their thumbnail underlay
whenever a scroll stopped inside the window, until the settle. The current
design removes the underlay and takes visibility from the virtualizer's
motion band, so a page is either in the band — and rendering — or waiting for
the settle.
