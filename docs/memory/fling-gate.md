# The fling gate, scroll churn, and the dwell fix

The in-view exemption shipped in `16bbe4a` regressed idle memory. The fix
is the dwell in `c9e73dc`. This records what the gate protects, how the
regression worked, why CI could not see it, and the fix.

## What the gate protects

The render effect in `crates/reader-runtime/src/components/formats/pdf/
canvas.rs` decides whether a page may start a raster. While the strip
moves (`settled == Some(false)`), an unpainted page stays on its upscaled
thumbnail underlay:

```rust
// The gate before the perf work (8f6b099)
if !painted.get() && settled.as_ref().is_some_and(|s| !s.get()) {
    if !(gw > 0.0 && gh > 0.0) {
        pdf().blit_thumb(&cid_effect, page);
    }
    return;
}
```

The gate is a memory device. Two facts of browser memory make it one:

1. A raster sets the page canvas to full page pixels (10–25 MB of backing
   store at reading zoom). The thumbnail path resizes the canvas to the
   thumbnail's dimensions, a few hundred KB.
2. The webview's resource cache grows to the peak of surfaces it has seen
   and does not hand that peak back at idle. Churn — large surfaces created
   and discarded every few frames — drives the peak up even though no
   single moment holds much.

The gate's comment records the measured history:

> a full-resolution rasterisation for every page a fling flies past
> creates, paints and discards a full-page surface every few frames, and
> that churn — not the mounted ceiling — is what pushes the webview's
> resource cache, and the footprint latched onto it, to its high-water
> mark.

## The regression

`16bbe4a` exempted pages inside the visible band from the gate.
Visibility is derived from the virtualizer's own model in
`in_view_signal` (`crates/reader-runtime/src/components/formats/pdf/
strip.rs`), read tracked so the crossing re-runs the effect:

```rust
// v1: crossing-based (regressed)
start + span >= scroll - IN_VIEW_MARGIN_PX
    && start <= scroll + viewport + IN_VIEW_MARGIN_PX
```

The mistake: `in_view` went true the moment a page crossed into the band.
During any real scroll every page crosses the band; a fling sweeps one
through in tens of milliseconds. Each one started a full-resolution raster
that was painted and discarded frames later — exactly the churn the gate
exists to stop, re-introduced for the pages the reader looks at. The
webview latched the higher footprint, and memory no longer dropped at
idle.

Nothing in CI could see it:

- The engine smoke suite has no DOM and no scrolling.
- The browser lifecycle suite asserts behaviour, not memory.
- The memory replay scrolls each pane once. The latch needs sustained
  scrolling; one wheel notch cannot produce it.

## The fix

The exemption requires the page to sit inside the band continuously before
it counts as visible:

```rust
const IN_VIEW_MARGIN_PX: f64 = 160.0;
const IN_VIEW_DWELL_MS: f64 = 120.0;

fn in_view_signal(
    virtualizer: Virtualizer,
    top: Signal<f64, LocalStorage>,
    size: Signal<f64, LocalStorage>,
) -> Signal<bool, LocalStorage> {
    let dwell_start = std::cell::Cell::<Option<f64>>::new(None);
    Signal::derive_local(move || {
        let scroll = virtualizer.scroll_offset().get();
        let viewport = virtualizer.viewport().get().main;
        // Layout-version carrier: a rebuild moves every offset without a
        // scroll.
        let _ = virtualizer.total_size().get();
        let start = top.get();
        let span = size.get().max(0.0);
        let inside = start + span >= scroll - IN_VIEW_MARGIN_PX
            && start <= scroll + viewport + IN_VIEW_MARGIN_PX;
        if !inside {
            dwell_start.set(None); // the clock restarts on every exit
            return false;
        }
        let now = in_view_now_ms(); // performance.now(), Date::now() fallback
        match dwell_start.get() {
            None => { dwell_start.set(Some(now)); false }
            Some(started_at) => now - started_at >= IN_VIEW_DWELL_MS,
        }
    })
}
```

The clock is the same monotonic pair the virtualizer's retention clock
uses. Behaviour by case:

| Situation | Behaviour |
| --- | --- |
| Fast fling (<120 ms in the band) | Never `in_view`; gate holds; thumbnail underlay; no churn |
| Slow scroll, the reader lingers ≥120 ms | `in_view` flips on a scroll tick; the page rasters while moving |
| Scroll stops inside the band before 120 ms | The 150 ms settle flips `settled`; the gate releases through `settled` as it always did |
| Zoom in flight | The effect's `anim` early-return runs before the gate |
| Page modes | No `in_view` signal; `visible_now` defaults true; page modes do not scroll the strip |

120 ms sits below the 150 ms settle on purpose: a page the reader actually
reads clears the dwell before the settle would have rendered it anyway, so
crispness is kept and the churn is not.

## Constants in this neighbourhood

| Constant | Value | Owner |
| --- | --- | --- |
| Scroll settle delay | 150 ms | virtualizer `scroll_end_delay` |
| `IN_VIEW_DWELL_MS` | 120 ms | `formats/pdf/strip.rs` |
| `IN_VIEW_MARGIN_PX` | 160 px | `formats/pdf/strip.rs` |
| `CLEANUP_EVERY` | 5 renders | `engine/state.ts` |
| `SWEEP_IDLE_MS` | 30 s | `engine/state.ts` |
| `RAW_IDLE_MS` | 2 s | `engine/state.ts` |
| `MAX_ZOMBIES` / `STRIP_SCROLL_GRACE_MS` | 12 / 120 ms | `zoom/config.rs` |
| `PAGE_RENDER_LIMIT` / `REALM_PAGE_LIMIT` | 2 / 2 | `renderer.ts`, `state.ts` |

## Commits

| Commit | Change |
| --- | --- |
| `16bbe4a` | Exemption, v1, crossing-based; caused the regression |
| `c9e73dc` | Dwell on the exemption; fix |
| `8b54337` | This document (first as `docs/fling-gate-retrospective.md`) |
