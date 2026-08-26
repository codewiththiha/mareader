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

`css_heights` is the shared measurement store. It seeds the virtualizer, receives measured page heights, and is rescaled during zoom/layout animation. Geometry queries themselves go through the virtualizer and the layout APIs rather than through a parallel app-local model.

## Continuous reader flow

1. `ReaderPage` builds one `Virtualizer` for the continuous surface.
2. `PageList` binds the scroll container, renders `v.items()`, and reports measured page heights back into both `css_heights` and the virtualizer.
3. Navigation sync uses the virtualizer for dominant-page tracking and page-to-scroll jumps.
4. Search reveal uses virtualizer offsets plus virtualizer scroll commands.
5. Fit and zoom rescale `css_heights` and apply the same rescale to the virtualizer so anchoring and mounted windows stay in sync.

## Thumbnail panel flow

The thumbnail sidebar is a separate grid virtualizer:

- width-aware row windowing lives in `virtual-list`
- DOM/reactive wiring lives in `virtual-list-leptos`
- panel-specific constants stay in `src/components/panels/thumbnails`

That keeps list and grid virtualization on the same geometry stack while letting each surface keep its own rendering policy.
