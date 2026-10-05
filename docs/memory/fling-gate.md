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
derives visibility from the virtualizer's own model (scroll offset,
viewport, item offsets) and is read tracked by the render effect:

| Situation | Behaviour |
| --- | --- |
| Page inside, or within `IN_VIEW_MARGIN_PX` of, the viewport at reading speed | Visible at once; renders while the strip moves |
| Same, while the strip moves faster than `FLING_PX_PER_MS` | Visible after `IN_VIEW_DWELL_MS` continuously in the band; the clock restarts on every exit |
| Fling stops inside the dwell window | A one-shot timer wakes the derive at the deadline; the page renders then |
| Page outside the band | Renders at the scroll settle |
| Zoom in flight | The effect's `anim` branch runs before the gate |
| Page modes | No `in_view` signal; pages render immediately |

The timer is what keeps the gate from stalling: visibility never depends on
another scroll event arriving. It is an `ArcTrigger`, one per page at a
time, at most one dwell long, so a wake after unmount notifies nothing.

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
| Scroll settle delay | 150 ms | virtualizer `scroll_end_delay` |
| `IN_VIEW_MARGIN_PX` | 320 px | `formats/pdf/strip.rs` |
| `FLING_PX_PER_MS` | 4 px/ms | `formats/pdf/strip.rs` |
| `IN_VIEW_DWELL_MS` | 60 ms | `formats/pdf/strip.rs` |
| `CLEANUP_EVERY` | 5 renders | `engine/state.ts` |
| `SWEEP_IDLE_MS` | 30 s | `engine/state.ts` |
| `RAW_IDLE_MS` | 2 s | `engine/state.ts` |
| `MAX_ZOMBIES` / `STRIP_SCROLL_GRACE_MS` | 12 / 120 ms | `zoom/config.rs` |
| `PAGE_RENDER_LIMIT` / `REALM_PAGE_LIMIT` | 2 / 2 | `renderer.ts`, `state.ts` |

## History

A first version exempted visible pages the moment they crossed into the
band. During a fling every page crosses, so each started a raster that was
discarded frames later; the webview latched the higher footprint. A 120 ms
dwell fixed the churn but left pages blurry on their thumbnail underlay
whenever a scroll stopped inside the window, until the settle. The current
design removes the underlay, renders immediately at reading speed, shortens
the mid-fling dwell and wakes on a timer.
