# Architecture

This repo now splits virtual scrolling into three layers.

## 1. `virtual-list`: pure geometry

`crates/virtual-list` owns the reusable math:

- list and grid layout contracts
- mounted-window selection from viewport + budget
- dominant-item selection
- offset and total-size queries
- anchor correction for measurement changes and uniform rescaling

It has no DOM, no framework coupling, and is the long-term public crate surface.

## 2. `virtual-list-leptos`: reactive adapter

`crates/virtual-list-leptos` wraps the geometry kernel in a Leptos-friendly adapter:

- `VirtualizerCore` is still pure and unit-testable
- `use_virtualizer` binds browser scroll containers and resize observers
- the public `Virtualizer` exposes reactive mounted items/rows, total size, padding, dominant item, scrolling state, and scroll-to APIs

This layer is responsible for DOM measurement flow, scroll scheduling, and keeping the geometry authoritative.

## 3. Reader app: policy + rendering

The app uses the adapter and keeps only app-specific policy locally:

- page gap and render budget constants
- toolbar inset and view-mode state
- page rendering, text/search overlays, and chrome
- measurement storage in `css_heights`

`css_heights` is the shared measurement store. It seeds the virtualizer, receives measured page heights, and is rescaled by the viewer engine on every frame of a zoom. Geometry queries themselves go through the virtualizer and the layout APIs rather than through a parallel app-local model.

## Reader motion principles

1. Zoom animates the LAYOUT. Every frame of the tween rescales the strips
   through the viewer engine, so the document genuinely resizes under the
   reader's eyes.
2. The engine holds the document point under the viewport centre exactly
   where it is while it does so — computed gap-aware, because page heights
   scale and the gap between pages does not. Nothing is captured before a
   zoom and nothing is restored after it.
3. Zoom never scales a frozen surface with a CSS transform. A transform
   scales the page gaps along with the pages, the layout deliberately does
   not, and the whole accumulated difference lands at once when the transform
   is swapped for real geometry — which reads as the document jumping.
4. Page hosts stretch the bitmap they already hold. Nothing re-rasterises
   while the scale is moving; the crisp render is issued once, at the settled
   scale, when the transition commits. A container follow is what makes that
   rule non-trivial: a sidebar slide or a window drag relays the layout out on
   every frame and holds its COMMIT until the size has been quiet, so the burst
   costs one render pass rather than one per frame.
5. The virtualizer's window never drives the page number mid-zoom — and it
   catches the page up when the transaction lands, which is what keeps a long
   held follow from leaving the counter on a page the reader has scrolled past.
6. Recently evicted virtual items become zombies briefly (bounded set,
   grace longer than the tween), bridging the window across a zoom.
7. Zombie items never trigger a new PDF render; they keep their DOM and
   their last bitmap until their grace expires.
8. Page turns and reader surfaces (popovers, search, toasts) do not
   fade, slide or bounce in — document content appears instantly.
9. Layout chrome (the sidebar width, overlays) may use short STRUCTURAL CSS
   transitions; decorative entrance keyframes do not come back. Whether they run
   at all is the reader's call: Settings → Layout holds the master switch, and
   Settings → Animations holds one switch per motion the reader models. Both are
   projected into `state::reader::Motion` by the shell, which is what every gate
   reads — the master is applied once, there, and nothing downstream asks twice.
10. The page host is sized by its inline width/height, which ARE the page
    geometry, so it never lets the flex engine renegotiate them — a shrink
    takes the width while the height stays and the paper visibly squishes.
    There is no reflow exception for a resize, and none is needed: the layout
    follows the container on every frame of a slide or a drag, so the host is
    never left wider than its line for a frame the reader can see. A
    hand-picked zoom that overflows the window keeps overflowing — that is the
    page scrolling rather than squishing, exactly as a fit page in the
    horizontal strip does.
11. PDF renders happen only at transaction boundaries, never per frame.
12. A switched-off animation never skips the CHANGE, only the frames. Each gate
    sits where the interpolation happens, not where the change is decided:
    `animation.rs` declines to tween, `follow_watcher` drops the per-frame posts
    of a *gated* burst (a window drag) and lands the end frame once it is quiet,
    `scroll_mode` resolves to `Instant`, and the rail's class list gains
    `no-slide`. So a frozen reader is not a reader with fewer features — it is
    the same geometry, arriving at once. The one resize that is never gated is
    the follow of the sidebar's own width: there the frames ARE the correctness
    (a page may not be wider than the box the flex engine gave it), and landing
    in the frame the container was measured is what keeps a frozen slide to a
    single step instead of a step followed by a correction.

## Continuous reader flow

1. `ReaderPage` builds one `Virtualizer` for the continuous surface.
2. `PageList` binds the scroll container, renders `v.items()`, and reports measured page heights back into both `css_heights` and the virtualizer.
3. Navigation sync uses the virtualizer for dominant-page tracking and page-to-scroll jumps.
4. Search reveal uses virtualizer offsets plus virtualizer scroll commands.
5. Zoom runs through one controller: commands resolve to a target, the tween relays the layout out through the engine frame by frame — `css_heights`, both strips and the page hosts all follow the live display scale — and the render scale catches up once, at the end.

## Thumbnail panel flow

The thumbnail sidebar is a separate grid virtualizer:

- width-aware row windowing lives in `virtual-list`
- DOM/reactive wiring lives in `virtual-list-leptos`
- panel-specific constants stay in `src/components/shell/sidebar/panels/thumbnails`

That keeps list and grid virtualization on the same geometry stack while letting each surface keep its own rendering policy.
