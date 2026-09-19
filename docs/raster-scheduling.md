# Raster admission and residency

The reader has two budgets, with different jobs:

- **Geometry:** the vertical and horizontal PDF virtualizers have a base mount budget of 12 items / 1.25 screens, with a 0.65-screen active pixel band. The PDF strip unions that with a **five-page warm window (current ±2)**, using the same virtualizer offsets. This can add up to four neighboring hosts beyond the base window, so even giant/zoomed pages have their next page mounted early. Outside both bands, Blank/Zombie items remain lightweight shells; retention is capped at three shells. Active hosts are seeded from intrinsic dimensions before first paint.
- **Reading continuity:** at page 2, pages 1–4 are eligible to be warm; at page 3, pages 1–5 are eligible. Page 5 starts warming before the reader gets halfway through page 3, rather than waiting for it to enter the viewport. Both directions are kept ready, with the current scroll direction receiving higher priority. Neighboring hosts keep their existing pixels while they remain in this window. Normal read-ahead is subject to the same byte budget, not an unbounded prefetch batch.
- **Pixels:** `RasterScheduler` admits work against a 192 MiB soft / 320 MiB hard allocation budget. These are initial policy values, not measurements of browser or process memory.

The existing Leptos/Tauri structure, geometry kernel, Rust/JS facade, theme pipeline and CSS stretch zoom remain in place. No new runtime dependency is required.

## Ownership and admission

`renderPageInternal` is private. Normal pages, committed zooms, missing-raw theme refreshes and scrub preparation use `renderPage`. Retained-raw page bakes, thumbnail renders/rebakes, idle prewarm and paper samples share the same lane.

The queue is a map keyed by resource owner. Replacing a queued request resolves its old promise immediately. Canceling an active request marks its ticket stale and asks pdf.js to cancel, but **does not release its reservation or canvas slot until its asynchronous cleanup finishes**. A same-id remount therefore cannot race the previous render. Every post-await page write checks the ticket, registration identity, document identity and render generation.

Admission includes resident surfaces plus reservations. Resident accounting deduplicates display/raw aliases and includes DOM canvases (including previews and scrub snapshots), retained raw pages and cached thumbnail canvases/bitmaps. Reservations conservatively allow six RGBA-sized surfaces per requested pixel: raw target, replacement display, filter/worker data, scratch, blend output and safety room. Identity renders over-reserve deliberately. Active outputs may also appear in the resident scan, which makes admission conservative rather than optimistic.

Optional raw copies and the byte-weighted thumbnail LRU are evicted under soft pressure. Visible display surfaces are not wiped to make room; full evictions occur when the render child leaves both the active band and the warm page window. If a page's requested pixels do not fit the hard limit, admission reduces its pixel ceiling. Requests that cannot fit even the minimum resolve as canceled rather than waiting forever. Pending or active owners are protected from optional-raw eviction.

A render uses a separate target and swaps only complete pixels into the display. Old pixels remain usable through cancellation, zoom and theme work. Temporary surfaces and `page.cleanup()` are released in `finally`, including context failure, thrown render, failed text extraction and stale completion. Shared bake scratch retains its DOM object but drops its backing store at release. Thumbnail cache entries never own shared scratch.

## Motion policy

`ScrollMotion` tracks CSS pixels per millisecond. The fling threshold is **2.5 screens per second**, not screens per millisecond. Its prediction is clamped to the scroller's extent. Direction reversals and displacements exceeding three screens increment a navigation diagnostic generation. Requests are invalidated using current geometry and owner/document generations rather than walking intermediate pages.

- Tracking (normal scrolling): admit one job, visible pages first and directional neighbors next. There is no scroll-end debounce before read-ahead. Requests have a 4 MP ceiling.
- Fling: admit no new raster work. Keep existing pixels and use cached previews where available; queued intent resumes on deceleration/settle.
- Settling: after 100 ms without scroll, admit two jobs; full page requests have a 4 MP ceiling.
- Idle: after a further 80 ms, admit two jobs; new page requests have an 8 MP ceiling, also bounded by the device's existing `PAGE_MAX_PIXELS` policy.

Visible pages rank before forward neighbors, backward neighbors, sidebar thumbnails and prefetch; prediction breaks ties. The predictor ranks mounted candidates; it does not create a second document geometry index or rasterize an unmounted predicted page. Obsolete work outside both the pixel band and the warm page window is removed on replan. The strip publishes its anchor page and axis to the scheduler, including before the first scroll event. Cached sidebar blits remain synchronous; prewarm and backdrop samples yield to foreground work.

Warm-page jobs also prepare selectable text and links. Otherwise an already-painted neighbor would enter view without a text layer and the component's same-scale fast path would never request one. Scroll-start/end effects do not replace an identical in-flight scale request.

CSS geometry follows zoom continuously. A full rerender is deferred until a committed scale differs by at least 12% from the last actual raster scale. The reference does not advance on skipped ticks, so repeated small changes eventually trigger refinement. The renderer does not allocate a zoom snapshot: the existing display is already preserved until replacement. An intentionally reduced raster is not automatically promoted solely by an idle timer; subsequent zoom/theme/page requests reconsider quality.

## Diagnostics

`PDFReader.stats().raster` reports resident/reserved bytes, budgets, queued/running work, dropped/canceled requests, pressure downgrades, peak admission accounting, motion, predicted offset, and full-render counters. Offscreen completions now include intentional warm-neighbor work and must not all be interpreted as waste. Append `?rasterDebug=1` to enable the small opt-in diagnostic HUD.

These numbers are **not process memory**. They exclude pdf.js decoded document/font/operator caches, the browser's internal GPU copies, WASM/JS heap overhead and allocator retention. Track Activity Monitor/browser process peaks separately. No claim that a 320 MiB admission ceiling makes the entire reader process 320 MiB is intended.

## Validation

GitHub's existing Web lane runs the new tests through `tools/test-engine-smoke.ts`, alongside the existing theme/scrub/thumbnail/teardown tests. No workflow or dependency change is required.

Deterministic scheduler tests cover priority, request replacement, a 100-page skipped path, active cancellation reservations, same-key exclusion, hard-limit downgrade, impossible admission, synchronous throws, paused work, teardown, giant/asymmetric pages, both scroll directions, reversal and jumps. Engine lifecycle tests cover asynchronous getPage/remount, failed text extraction, synchronous render failure and missing canvas context. Existing pixel tests verify theme and scrub output. Read-ahead regressions check the shared Rust/JS radius, current/neighbor priority, giant pages, page 5 completing offscreen during Tracking, prebuilt text/links, fast-jump suppression, and horizontal cold-start priority. These execute the production scheduler and bundled engine in GitHub CI.

For device testing, use a long mixed-size PDF and record downward/upward flings, rapid reversal, page 5 → 500 jumps, repeated zoom commits, zoom + scrolling and theme scrub. Confirm no new full renders start during fling, obsolete jobs disappear, reservations drain after cancellation, and the landing page becomes readable promptly. Tune budgets and motion thresholds against those measurements; stub tests cannot establish real WKWebView GPU peaks or perceived smoothness.
