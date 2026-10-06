# Virtualizer rewrite: motion-led banding, release-on-eviction

Branch `perf/virtualizer-motion`, off `main` (`b741cb5`). Scope: `crates/virtual-list`
(the `no_std` geometry kernel) and `crates/virtual-list-leptos` (the Leptos adapter
that the PDF and reflow strips mount frames through). Public endpoints stay
source-compatible; new capability is additive.

## The two complaints, diagnosed in the current code

1. **Blanks during ordinary scrolling.** `VirtualizerCore::render_range`
   (`crates/virtual-list-leptos/src/engine.rs:489`) makes the content band
   *symmetric around the viewport* and *speed-blind*: `viewport ±
   render_screens × viewport`, intersected with the mount window. A mount
   outside the band renders `VirtualItemState::Blank`, and the band only ever
   covers what the scroll position already says is near. Two consequences:
   a page one inch below the fold is a placeholder even at a gentle scroll, and
   nothing in the virtualizer asks for its content any earlier than the scroll
   reaches it — so "fill" is always a race the raster lane loses. The
   direction of travel, which is the only information that says *which*
   placeholder to fill first, is never consulted.
2. **A cache that pays nothing.** `retention.rs` keeps evicted items mounted for
   `Frames { frames: 4 }` / `Grace { ms }` — a *wall-clock and frame* bridge that
   runs on every eviction, at rest or at speed (`RetentionPolicy::bridges()` only
   checks `max > 0`). The doc-comment justifies it with "fast scrolls"; the code
   cannot tell a fast scroll from a nudge, so a one-line wheel step holds DOM,
   layout and the page's content alive for four frames × 120 ms ceilings and a
   `max`-item pool, and `retain_evicted` only ever bounds the *count* — never the
   fact that it engaged at all. Nothing on the other side releases content:
   evicting a row drops its DOM row and its canvas *registration*, but the
   engine-side per-page state is freed at session end (`documentPages: 40` with
   4 rows mounted in the 2026-10-05 `:446` lifecycle dump is that gap).
   Memory up, smoothness unchanged — exactly the report.

There is no velocity model anywhere in either crate (a repo-wide grep for
`velocity|speed|lookahead|prefetch` in `crates/virtual-list*/src` returns one
doc-comment mention and nothing else). The one place that behaves like look-ahead
is `crates/reader-runtime/src/effects/reader/blend_backdrop.rs`, which samples the
paper colour of pages the reader is approaching — the samples that
`lookaheadSamplesActive` counts in the lifecycle suite. So "the speed detection is
a mock" is accurate: a placeholder policy and a colour sampler exist; a motion
model does not.

## Target design

### 1. A real motion model (`virtual_list::motion`, new, pure, `no_std`)

Sample the scroller on the frame chain the adapter already owns. Per sample:
`Δpx / Δt` → frame-rate-independent EMA of signed velocity (`α = 1 − e^(−Δt/τ)`,
τ = one frame), plus a decay so a scroll that stops emitting events reads as
stopped instead of frozen at the last speed. Derived, all of it honest arithmetic
about the scroller:

- `direction()` — sign with hysteresis (a flip needs to clear a fraction of the
  enter threshold, so a jittery trackpad does not oscillate the lead side);
- `seeking()` — a two-threshold state machine in the shape react-virtuoso uses for
  scroll-seek mode (`enter: |v| > 200`, `exit: |v| < 30` px/s, [api-docs]):
  placeholders are a *fast scroll* mode, entered on a fling and left when the
  reader slows; below the enter threshold the band simply covers the window;
- `lead(viewport, fill_latency)` — the band's leading pad is
  `|v| × fill_latency`, clamped to `[min_lead, max_lead screens]`. That is the
  production rule for "no blanks while scrolling", derived rather than guessed:
  in the time it takes to fill one page, the reader travels this far, so this is
  how far ahead must already be warm. `fill_latency` is the caller's measured fill
  cost (the raster lane's recent p50), not a constant;
- `trail(viewport)` — the trailing pad stays small (≈ a quarter screen plus the
  budget's overscan): the reader can only come back at the speed they came, and
  the DOM behind the viewport is what costs memory;
- `min_items_ahead` — a count floor under the pixel pad, because a page can be
  taller than the pad (react-virtuoso added `minOverscanItemCount` for exactly
  this reason: pixel padding is not enough for tall or dynamic content);
- `priority(index)` / `predicted()` — per-item fill priority (0 = visible now,
  1 = in the lead while seeking, 2 = behind the viewport) and the index the
  viewport is expected to land on after `fill_latency`. The raster lane consumes
  these, which is the whole point: the blanks fill in the order the reader is
  moving toward, not document order.

Everything is `#[derive(Debug, Clone, Copy, PartialEq)]` over `f64` and unit-tested
on the host, including: a nudge never engages placeholders; a fling engages them
and disengages once under the exit threshold; the lead grows linearly in speed and
saturates at `max_lead`; a stopped scroller decays to zero within a frame or two.

### 2. Band = window ∩ (viewport + directional lead), and the band never blanks
   anything the reader can see (the current invariant, kept), but at rest the
   band **is** the window: `seeking() == false` ⇒ every mounted item renders.
   That removes "blanks in normal scrolling" as a class — the placeholder mode is
   opt-in per frame by measured speed.

### 3. Retention is earned by motion, not granted by a clock. A bridge is taken
   only while `seeking()` (the window jitters around a fling) or while a geometry
   commit is in flight (zoom, which is why `set_retention_policy` exists). At
   rest, an evicted row unmounts in the same tick — `RetentionPolicy::Immediate`
   behaviour regardless of what the policy says, and `retainedVirtualItems`
   decays to 0 without waiting for its deadline. `RetentionPolicy` keeps its
   shape (consumers construct it) but gains `MotionGated`, which becomes the
   default: `bridges_now(motion, commit)` decides per publish.

### 4. Content dies with the frame; the queue decides who goes first.
   A page's rasters, raw bitmap and canvas are released by the unmount the band
   now causes promptly (`unregisterPage` → `releasePageSurfaces`, which zeroes
   the backing stores), while the **measurement** — the item's size — stays with
   the layout as long as the layout keeps the item: 16 bytes that keep the
   scrollbar and every anchor honest (TanStack keeps `itemSizeCache` for the same
   reason; a reader must not keep rasters for it). An earlier draft of this
   round added a release *event* (`on_release` plus a bounded ledger) so caches
   could drop a page one tick sooner than the DOM did. Nothing needed it: every
   per-page surface is already owned by the frame, so the event had no honest
   consumer and it was deleted rather than left as a hook. What was missing was
   not an earlier release but an ordered fill — see §6.

### 6. Fill order is the lane's, and it is derived, not configured. The page
   lane runs two rasters per session, so the cost of one queued page is roughly
   the cost of the frame the reader is waiting for. Issued in mount order, the
   page under the reader's eyes waits behind the overscan queued around it and
   shows a blank at a speed nobody would call fast. So the strip hands every
   request a `rank`: the band's own class first (`FillPriority::rank`: visible,
   ahead, behind, warm) and then the distance from `landing_index()`, so a page
   the reader is travelling toward outranks one they have left, and two pages in
   one class compete on which pair of eyes reaches it first. A raster already
   running is never preempted — abandoning it buys a blank, not a bitmap — but a
   page that leaves the band while still unpainted has its queued request
   cancelled, because a slot spent on it is a slot the visible pages cannot use.

### 5. Measurement cadence. Keep the coalesced pre-paint flush, but a mount whose
   measured size differs from the estimate by more than `eps` re-windows within
   the same frame instead of waiting for the scroll event that would have
   followed — the "slow one" fix, and the reason the correction can be applied
   without fighting momentum (the write is only made while `!seeking()`, exactly
   like the iOS-deferral rule TanStack added in 2026: layout always lands,
   the scroll correction waits for the gesture to settle).

## Endpoint policy (nothing may break at the call sites)

Every item in `crates/virtual-list-leptos/src/lib.rs`'s re-export list and every
`pub fn` on `Virtualizer`/`VirtualizerCore` keeps its name, argument types and
return type. `VirtualizerOptions` keeps its fields and builder methods; new knobs
arrive as new builder methods (`pipeline`) and new read-only getters
(`motion_engaged`, `motion_speed`, `fill_priority`, `landing_index`). The kernel
keeps `Strip`, `Layout`, `ListLayout`, `GridLayout`, `GridSpec`,
`GridDimension`, `window_for`, `Window`, `Viewport`, `Budget`, `Overscan`,
`Align`, `AnchorPolicy`, `pin_at`, `correct`, `rescale_anchor`,
`subpixel_factor`, `SUBPIXEL_FACTOR`.

## Phase plan (each phase lands green on CI before the next)

1. `virtual-list::motion` (pure estimator + band math), unit tests in the crate
   and a `tests/motion_band.rs` suite for the policy. `cargo test -p
   virtual-list` runs them in CI.
2. Adapter: the core owns `Motion` + `Pipeline` and *stores* the band
   (`rewindow` evaluates it, so every getter stays `&self`), `retention.rs`
   (motion-gated bridge), `virtualizer.rs`/`hook.rs` endpoints preserved.
3. Consumers: the strip reads the band for what carries content and the band's
   priority for the engine's page-lane `rank`; the lane inserts by rank and
   pumps a cancelled job out instead of parking it at its head.
4. Gates: `tests/browser/lifecycle.mjs` — the look-ahead stage and the retention
   ceilings get *tighter* assertions (placeholder mode must not engage at normal
   speed; `retainedVirtualItems` must fall to 0 at rest without waiting for the
   deadline), and `tools/check-memory-discipline.mjs` keeps every virtualizer
   disposal in place.

[api-docs]: https://virtuoso.dev/react-virtuoso/api-reference/common/

## As landed, in `virtual-list` (kernel)

Refinements that came out of writing it, so the plan above and the code agree:

- `Motion::update(offset, now_ms)` is *only* the measurement; it deliberately
  knows nothing about the pipeline. Engagement is evaluated by
  `Motion::band(offset, viewport, overscan_px, &Pipeline)`, which owns the latch
  and returns `BandWindow { active: BandRange, placeholder, lead_px, trail_px,
  speed_px_s, direction }` — a pixel band, because the kernel has no idea what
  an index costs; the engine turns it into indices with
  `Layout::overlapping` and intersects it with the mount window.
- The gates live on the pipeline, not in the config: `Pipeline::enter_px_s` =
  `max(config.enter_floor_px_s, capacity_px_s)`, `Pipeline::exit_px_s` =
  `enter × config.hysteresis` (0.4). There is no separate exit constant to
  remember, and on a machine that can fill 10 000 px/s an ordinary 3 000 px/s
  scroll is a *proved* non-event, not a configured one.
- `lead` is `speed × fill_ms`, floored at `min_lead_screens` (0.5), capped at
  `max_lead_screens` (2.0), and never below the mount policy's own overscan;
  `trail` is `trail_screens`, capped by `lead`.
- Fill order is a `u32` sort key handed to the engine lane
  (`pdf-engine::render_page(..., rank)`, `PageLane.push(rank, run)`), derived by
  `rank_signal` from `FillPriority::rank() × 2¹⁶ + |index − landing_index()|`.
  Read untracked at issue time: a rank change says who goes first, not what has
  to be drawn, so re-running a render effect for it would restart rasters.
- `blend_backdrop`'s "am I moving" input becomes `motion_drifts` — true while
  engaged, `false` once settled and slower than `drift_eps`, and nothing else —
  replacing any speed *ratio* a caller might have invented. The engine-side
  `retention::RetentionPolicy::MotionGated { max }` replaces `Frames` (whose
  frame count was a guess) and becomes the strip default; `Grace { ms, max }`
  stays for callers with a known wall-clock window, which is a zoom commit.
- No `render_screens`-as-ceiling: a caller that sets a band asks for a floor
  under the motion-derived band, never a cap over it. `render_band(0)` keeps its
  meaning, which is that the mode is off.

## Status

All four phases are landed on `perf/virtualizer-motion`. What the reader sees:
at reading speed nothing mounted is a placeholder and every page renders in the
frame it mounts; a fling earns a lead instead of a fixed pad, pays for it by
blanking only what it flew past, and hands the queue back the work it no longer
wants; at rest the bridge dissolves in the same frame the motion estimate
decays. `CI` and `Deep CI` green on the gated SHA; the page lane's queue and
active slots stay drained at quiescence, which `tests/browser/lifecycle.mjs`
asserts.

<!-- // only the changed file was rewritten -->
