# The fling gate, scroll churn, and the dwell fix — a retrospective

The PDF perf work of 2026-09-29/30 shipped an in-view exemption to the
strip's fling gate so pages raster while the reader looks at them
(`16bbe4a`). Its first version regressed memory: after a reading session
the footprint no longer came back at idle. The fix is a DWELL on the
exemption (`c9e73dc`). This document records, in full, what the gate
protects, how the regression worked, why nothing in CI could see it, and
how to diagnose and avoid this class of bug — so the next person does not
re-open the hole.

## 1. What the fling gate is, and why it exists

`PdfPageCanvas`'s render effect (`crates/reader-runtime/src/components/
formats/pdf/canvas.rs`) decides whether a page may START a raster. While
the strip still moves (`settled == Some(false)`), an UNPAINTED page is
held on its upscaled thumbnail underlay instead:

```rust
// The gate, as it stood before the perf work (8f6b099):
if !painted.get() && settled.as_ref().is_some_and(|s| !s.get()) {
    if !(gw > 0.0 && gh > 0.0) {
        pdf().blit_thumb(&cid_effect, page);
    }
    return;
}
```

The gate's own comment records the measured history that built it — read
it before touching this code:

> *"a full-resolution rasterisation for every page a fling flies past
> creates, paints and discards a full-page surface every few frames, and
> that churn — not the mounted ceiling — is what pushes the webview's
> resource cache, and the footprint latched onto it, to its high-water
> mark."*

Two facts about browser memory make this true:

1. **Full-res surfaces are large.** A raster sets the page canvas to full
   page pixels (`renderer.ts`: `target.width = pxW; target.height = pxH`)
   — typically 10–25 MB of backing store per page at reading zoom. The
   thumbnail path instead blits a small bitmap and `blitInto` RESIZES the
   canvas to the thumbnail's dimensions, so a gated page's backing store
   is a few hundred KB.
2. **The footprint latches.** The webview's resource cache grows to the
   peak of surfaces it has seen and does not hand that peak back promptly
   at idle. Churn — big surfaces created and discarded every few frames —
   drives the peak up even though no single moment holds much.

So the gate is a MEMORY device, not a CPU one: it keeps pages the strip is
only SWEEPING PAST from ever owning a full-res surface. The visible cost —
pages look blurry until the 150 ms scroll settle — was the accepted
trade-off… and was the blur the perf set out to fix.

## 2. What the perf work changed (and the v1 mistake)

Commit `16bbe4a` ("fix(reader): render in-view pages during scroll")
exempted pages inside the visible band from the gate. Visibility is
derived from the virtualizer's OWN model — scroll offset, viewport extent
and the item's offsets — by `in_view_signal` in
`crates/reader-runtime/src/components/formats/pdf/strip.rs`; no
IntersectionObserver, agreement with the layout by construction. The gate
became:

```rust
let visible_now = in_view.as_ref().is_none_or(|v| v.get());
if !painted.get() && !visible_now && settled.as_ref().is_some_and(|s| !s.get()) {
    if !(gw > 0.0 && gh > 0.0) {
        pdf().blit_thumb(&cid_effect, page);
    }
    return;
}
```

**The v1 mistake:** `in_view` went true the moment a page's box crossed
into the band (plus a 160 px margin). During any real scroll, EVERY page
crosses the band — a fling sweeps a page through in tens of milliseconds.
Each one immediately started a full-resolution raster: created, painted,
discarded a few frames later when the page unmounted or was superseded.
That is exactly the churn quoted above, re-introduced for the pages the
reader looks at. Result: the webview latched a much higher footprint, and
**memory no longer dropped at idle** — the regression the user reported.

Why nothing caught it:

- The engine smoke suite has no DOM and no scrolling.
- The browser lifecycle suite scrolls, but asserts BEHAVIOUR (renders
  progress, lane gauges, window bounds), not memory.
- The memory replay (`tools/measure-split-return.mjs`) scrolls each pane
  ONCE (~1600 px). The latch needs sustained scrolling to accumulate
  churn; one wheel notch cannot produce it. Its numbers barely moved
  between the affected commit and its parent.

## 3. The fix: dwell (`c9e73dc`)

The exemption now requires the page to DWELL inside the band
CONTINUOUSLY before it counts as visible:

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
            dwell_start.set(None);   // the clock restarts on every exit
            return false;
        }
        let now = in_view_now_ms();  // performance.now(), Date::now() fallback
        match dwell_start.get() {
            None => { dwell_start.set(Some(now)); false }
            Some(started_at) => now - started_at >= IN_VIEW_DWELL_MS,
        }
    })
}
```

Why the numbers line up:

| Situation | Behaviour |
| --- | --- |
| Fast fling through the band (<120 ms in it) | never `in_view` → gate holds → thumbnail underlay → **no churn** (the pre-perf memory profile) |
| Slow scroll / the reader lingers ≥120 ms | `in_view` flips on a scroll tick → the effect re-runs (the read is TRACKED) → the page rasters while moving → crisp, no blur |
| Scroll stops inside the band before 120 ms | the 150 ms settle flips `settled`; the gate passes through `settled` anyway and the page rasters — same path as always |
| Zoom in flight | the effect's `anim` early-return runs before the gate; no raster either way |
| Page modes (single/fit) | the strip passes no `in_view` (`Option::None` → `visible_now == true`), but page modes don't scroll the strip; the gate behaves as before |

The dwell (120 ms) sits BELOW the scroll settle (150 ms) on purpose: a
page the reader actually reads clears the dwell before the settle would
have rendered it anyway, so crispness is kept and the churn is not. The
clock is the same monotonic pair (`performance.now()` / `Date::now()`
fallback) the virtualizer's retention clock uses.

## 4. Diagnosis playbook for this class of bug

Symptom: memory high at idle after a change to rendering/scroll paths;
earlier commits dropped back down.

1. **Separate holders from latches.** Run `PDFReader.stats()` in the live
   frame: if `sessionsLive > 0` at the library route, a session is still
   held open (warm/recycle policy — see `docs/runtime-split.md` §slot
   states), and the engine gauges (`pageCanvasBytesEst`,
   `thumbnailRasterBytesEst`, `rawRetentionBytesEst`,
   `pooledIntermediateBytesEst`) name what it holds. If `sessionsLive ==
   0` with memory still high, the holder is not engine state: it is the
   browser's latched resource footprint (this bug) or runtime-side
   retention (WebKit above all — see `docs/memory-baseline.md`).
2. **Ask what the change does DURING MOTION.** Idle-state audits miss
   churn bugs entirely: the surfaces are gone by the time you look, and
   only the latched peak remains. For any change that starts work during
   scroll/zoom/animation, enumerate the surfaces it creates PER ITEM
   SWEPT PAST, and multiply by scroll throughput.
3. **Reproduce with sustained scrolling.** The shipped harnesses scroll
   once; churn needs a storm. Loop the wheel in the replay harness (or
   drive `mouse.wheel` for seconds at a time) and sample PSS before,
   during and 30–60 s after. A footprint that rises during scroll and
   does not come back at idle confirms the latch.
4. **Check the gates you bypassed.** Every gate on this strip exists
   because a measured regression demanded it; the reason is in the
   comment above the gate. Bypassing or exempting a gate must preserve
   the property it protects, or pay for it explicitly.

## 5. Prevention rules

1. **Never start a full-res raster for an item the strip is only sweeping
   past.** An exemption from the fling gate must carry a time condition
   (dwell) or an equivalent commitment check. Crossing the band is not
   commitment.
2. **Band-crossing signals must be DWELLED before they authorize expensive
   work.** Cheap reactions (CSS classes, thumbnail blits) may fire on the
   crossing itself; surface-allocating work may not.
3. **Memory regressions need a scroll-heavy measurement.** If a change
   touches what renders during motion, extend the replay workload before
   trusting its numbers; one wheel notch measures teardown, not churn.
4. **The gate's invariants are load-bearing.** The lane paces starts, the
   generation guards drop superseded rasters, the settle renders the rest
   — the dwell leans on all three. Removing any of them re-opens this bug.

## 6. Timing and sizing constants in this neighbourhood

| Constant | Value | Owner | Role |
| --- | --- | --- | --- |
| scroll settle delay | 150 ms | virtualizer (`scroll_end_delay`) | `settled` flips true; the gate releases all mounted pages |
| `IN_VIEW_DWELL_MS` | 120 ms | `formats/pdf/strip.rs` | continuous in-band time before the exemption counts a page visible |
| `IN_VIEW_MARGIN_PX` | 160 px | `formats/pdf/strip.rs` | slack around the viewport in which a page can count as visible |
| `CLEANUP_EVERY` | 5 renders | `engine/state.ts` | pdf.js `cleanup()` cadence while rendering |
| `SWEEP_IDLE_MS` | 30 s | `engine/state.ts` | idle sweep: pdf.js cleanup + scratch/pool drain |
| `RAW_IDLE_MS` | 2 s | `engine/state.ts` | how long an unbaked raw canvas survives a theme change |
| `MAX_ZOMBIES` / `STRIP_SCROLL_GRACE_MS` | 12 / 120 ms | `zoom/config.rs` | virtualizer zombie retention |
| `PAGE_RENDER_LIMIT` / `REALM_PAGE_LIMIT` | 2 / 2 | `engine/renderer.ts`, `engine/state.ts` | per-session and realm-wide raster caps |
| `READER_RECYCLE_HEAP_MAX` / `READER_RECYCLE_PANES_MAX` | 320 MiB / 1 pane | `src/app/manager.rs` | when a returning reader frame is retired instead of recycled |

## 7. Commits

| Commit | Change |
| --- | --- |
| `16bbe4a` | fix(reader): render in-view pages during scroll — the exemption, v1 (crossing-based; caused the regression) |
| `e3446d1` | perf(engine): pace page rasters across panes — realm-wide raster cap, theme paths laned, unregister sweep removed (quiescence paths verified) |
| `93ae5af` | perf(engine): read bake pixels in the worker — theme readbacks off-thread |
| `c3c13cb` | docs(reader): record the paced raster lane |
| `a8fddd3` | fix(engine): never pin a session in the lane registry — WeakRef registry, disposed guards |
| `c9e73dc` | fix(reader): dwell in view before a page rasters — the fix this document records |
