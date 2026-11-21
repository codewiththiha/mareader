# UI Layering (Z-Order)

> **Status:** enforced (`crates/ui-egui/src/z_order.rs`, `cargo xtask z-order`) · **Last updated:** 2026-11-15

Every floating surface in `crates/ui-egui` declares a semantic `z_order::Tier`
instead of picking a raw `egui::Order`. This document records why raw egui
orders fail on their own, which tier every surface belongs to, the pairwise
stacking invariants that must hold, and how the policy is enforced at compile
time, runtime, and in tests.

## Why raw `egui::Order` is not enough

`egui` provides five coarse `Order` buckets (`Background < Middle < Foreground < Tooltip < Debug`)
with four subtle mechanics that produce z-index inversions if individual widgets
pick raw orders ad hoc:

1. **Coarse buckets force distinct roles to share one bucket.** `egui::Popup`
   hardcodes `PopupKind::Menu | PopupKind::Popup => Order::Foreground` with no
   order setter, and `egui::Modal` defaults to `Order::Foreground` (and hosts
   `ComboBox` popups inside Settings and Word List). Page-anchored hover/AI
   cards, floating tools, the floating overlay sidebar, anchored popovers,
   modal dialogs, and modal-hosted dropdowns therefore share
   `Order::Foreground`, while pane marks, pane bars, and window-edge chrome
   share `Order::Middle`.
2. **`Area::begin` promotes on click, drag, press, or first appearance.** Any
   `Area` automatically calls `move_to_top(layer_id)` whenever it is clicked,
   dragged, pressed, or transitions from hidden to visible. In `Areas::end_pass`,
   layers are stably sorted by `(layer.order, wants_to_be_on_top.contains(layer))`.
   Without an end-of-frame tier enforcer, clicking a lower-priority surface in
   the same bucket pulls it in front of a higher-priority surface (for example,
   clicking a modal dialog drops its own `ComboBox` behind it, clicking a
   floating dictionary pulls it over the floating sidebar, or a newly revealed
   page card jumps in front of the floating dictionary).
3. **`ctx.layer_painter` paints after every `Area` in the same `Order` bucket.**
   `GraphicLayers::drain` emits `Area` layers in `area_order` first and then
   iterates the remaining `order_map` HashMap entries. A passive `layer_painter`
   in `Order::Middle` therefore paints *on top of* every `Area` in
   `Order::Middle` (and in arbitrary hash order relative to other bare
   `layer_painter`s in the same bucket). Passive pane marks (`page-bar` progress
   line and counter pill) must paint directly onto `LayerId::background()` after
   document pages so they stay below all `Order::Middle` and `Order::Foreground`
   surfaces with no frame-0 sizing pass.
4. **Hover-reveal hit-testing must respect floating surfaces.** Edge-triggered
   chrome (`titlebar`, `page-bar`, closed overlay `sidebar-float`) queries
   `widgets::bare_pointer(ctx, own)` (or `z_order::is_above_tier` for the
   closed sidebar edge strip so hovering the title bar still runs the edge
   dwell timer). Any floating tool or modal above a split seam blocks
   hover-reveal beneath it.

## The stacking tiers (back → front)

`crates/ui-egui/src/z_order.rs` defines 17 `Tier` values in strict back-to-front
order:

| Rank | `Tier` | `egui::Order` | Kind | Surfaces |
|---:|---|---|---|---|
| 0 | `Canvas` | `Background` | Panel / `canvas_painter` | `CentralPanel` (PDF/EPUB panes, Library, Dictionary page), docked `SidePanel`, passive `page-bar` progress line & page counter pill (appended to `LayerId::background()` after pages) |
| 1 | `PaneMark` | `Middle` | Non-interactable `Area` | `floating-label` (per-pane chapter/title label) |
| 2 | `PaneBar` | `Middle` | Interactable `Area` | `("page-bar", pane)` hover-revealed scrubber & bookmark bar |
| 3 | `WindowChrome` | `Middle` | Interactable `Area` | `titlebar`, background `("import-run", id)` cards |
| 4 | `AttachedPill` | `Middle` | Interactable `Area` | `toast-island` (tucks its bottom 4 px seamlessly behind `read-aloud-bar` while staying above `page-bar` and `WindowChrome`) |
| 5 | `ChromeOverlay` | `Middle` | Non-interactable `painter` | `dict-edge-hint`, `lift-ring`, `lift` pane drop-target highlight (sits at the top of `Middle`, above pane chrome and strictly below page cards, floating tools, & modals) |
| 6 | `PageCard` | `Foreground` | Interactable `Area` | Page-anchored `Bubble` cards (`dict/card` hover lookup, `study/bar` selection toolbar, `study/card` & `study/note` cards, `footnote`, `scripts`), `page_ai/menu` popups (`popup`, `page_popup`), study mode's `("study-veil", pane)` frosted veil over locked pages, which holds the deck at the gate, its `("study-pill", pane)` next-deck pill and Reactive mode's `("study-recall", pane)` recall popover — above page chrome, strictly below floating tools |
| 7 | `FloatingTool` | `Foreground` | Interactable `Area` | `dict-window`, `dict-edge-tab`, `park::reveal` (`dict-window-edge-reveal`, `read-aloud-edge-reveal`), `read-aloud-bar`, `read-aloud-orb`, `read-aloud-ask`, `find-bar`, `study-picking`, `lift-card` |
| 8 | `Sidebar` | `Foreground` | Interactable `Area` | Floating overlay `sidebar-float` — slides cleanly over floating tools while staying below popovers and modals |
| 9 | `Popover` | `Foreground` | Interactable `Area` / `egui::Popup` | `study-mark-menu`, `widgets::submenu`, `library-search` suggestions, and every non-modal `egui::Popup` (titlebar popovers, view menu, context menus, dictionary pickers, read-aloud speed/options menus) |
| 10 | `Modal` | `Foreground` | `Dialog` (`egui::Modal`) / lightbox `Area` | `settings-window`, `word-list-window`, `shortcuts`, `palette`, `library-sheet`, `remove-alert`, `picture viewer` (`image_viewer`, also sets `set_modal_layer`) |
| 11 | `ModalPopover` | `Foreground` | `egui::Popup` (dynamic) | `ComboBox` dropdowns and popups opened while a `Modal` is active |
| 12 | `Notification` | `Tooltip` | Interactable `Area` | `toasts` corner stack, `toasts-open` expanded notification list |
| 13 | `Tooltip` | `Tooltip` | Non-interactable `Area` / `painter` / egui Tooltip | Standard hover tooltips, `dict-edge-peek`, `read-aloud-bubble`, `lift-target`, `library-carried`, `library-ghost`, `library-panel-drop`, `library-panel-tree-drop` |
| 14 | `WindowLights` | `Tooltip` | Non-interactable `painter` | `lights` (macOS traffic-light backings / non-Mac stand-in dots) |
| 15 | `PostEffect` | `Tooltip` | Non-interactable `painter` | `film-grain` full-window grain pass |
| 16 | `Transition` | `Debug` | Non-interactable `painter` | `crossfade` theme-switch snapshot |

## Core stacking invariants

The tier ordering guarantees every critical pairwise rule:

- **Split-pane bars stay behind floating tools:** Every pane's `page-bar`
  (`PaneBar`, `Order::Middle`) and passive counter/progress marks (`Canvas`,
  `Order::Background`) sit strictly below `dict-window`, `read-aloud-*`, and
  `find-bar` (`FloatingTool`, `Order::Foreground`) across any split layout.
- **Hover cards and page AI cards stay behind floating tools:** Page-anchored
  word lookup bubbles, highlight/note bubbles, selection toolbars, and page-AI
  menus (`PageCard`) sit above page chrome (`PaneBar`, `WindowChrome`) and
  strictly below `FloatingTool` (`dict-window`, `read-aloud-bar`, `find-bar`).
- **Floating sidebar slides over floating tools, below popovers & modals:**
  `sidebar-float` (`Sidebar`) renders cleanly over `dict-window` and other
  `FloatingTool` panels, and the floating sidebar inset does not clip the
  dictionary's stage rect (so the dictionary body stays intact behind the
  sliding rail). Context menus and dropdowns (`Popover`) and dialogs (`Modal`)
  still sit above `Sidebar`.
- **Popovers stay above their hosts:** Any `egui::Popup` or custom popover
  (`Popover`) stays above `WindowChrome`, `PageCard`, `FloatingTool`, and
  `Sidebar`, even if the user clicks the host panel.
- **Modals cover everything modeless, while hosting their own dropdowns:**
  `Modal` covers all `WindowChrome`, `PageCard`, `FloatingTool` (including
  parked `park::reveal` and `dict-edge-tab` handles), `Sidebar`, and pre-modal
  `Popover`s. Any popup opened while a `Modal` is active is classified as
  `ModalPopover` and stays strictly above that `Modal`.
- **Sibling floating tools still focus naturally:** When multiple `FloatingTool`
  panels are open together (such as `dict-window` and `find-bar`), clicking one
  raises it above its sibling while keeping both below `Sidebar`, `Popover`,
  and `Modal`.
- **Notifications stay legible without breaking attached pills:** Corner
  `toasts` (`Notification`) sit above chrome, floating tools, sidebar, and
  modals; `toast-island` (`AttachedPill`) sits above `page-bar` and
  `WindowChrome` while tucking cleanly behind `read-aloud-bar` (`FloatingTool`).
- **Closed layers are never revived:** `z_order::enforce` promotes only layers
  registered during the current pass or actively open via `Popup::is_id_open`,
  so closing a modal or popup immediately restores hit-testing underneath it.

## Rules for adding or modifying UI surfaces

1. **Never name raw `egui::Order` outside `z_order.rs`.**
2. **Create floating areas with `z_order::area(ctx, Tier::..., id)`** instead of
   `egui::Area::new(...)` or `egui::Window::new(...)`.
3. **Create modals with `modal::Dialog`** (or `z_order::area(..., Tier::Modal, ...)`
   plus `set_modal_layer` for custom full-window lightboxes).
4. **Create painter overlays with `z_order::painter(ctx, Tier::..., id)`** (or
   `z_order::canvas_painter(ctx)` for passive per-pane canvas marks). The single
   allowed direct `ctx.layer_painter` exception is `focus.rs`, which paints the
   keyboard focus ring directly onto the focused widget's own layer.
5. **Register `Popup::from_response(...).open(bool)` popups with
   `z_order::register_popup(ctx, response.layer_id)`.** Standard memory-backed
   popups (`Popup::menu`, `Popup::context_menu`, `Popup::from_toggle_button_response`,
   `widgets::combo`) are detected automatically via `Popup::is_id_open`.
6. **Never call `ctx.move_to_top(...)` directly.** `z_order::enforce(ctx)` runs
   at the end of `MareaderApp::ui` (`app/frame.rs`) and owns all within-bucket
   promotion without triggering idle repaints. An area that comes up under
   cards already open, such as study mode's veil under the page menu, calls
   `z_order::keep_under(ctx, tier, id)` before it shows.
7. **Guard hover-reveal strips with `widgets::bare_pointer(ctx, own)`** (or
   `z_order::is_above_tier` for the closed floating sidebar edge strip) so
   hovering through a floating window, popover, or modal never summons hidden
   edge chrome behind it.

## Enforcement

- **Static gate (`cargo xtask z-order`, wired into `cargo xtask ci` and CI):**
  `xtask/src/z_order.rs` scans all of `crates/ui-egui/src` and fails on raw
  `Order::*`, `.order(`, `Area::new(`, `Window::new(`, `Modal::new(` (outside
  `modal.rs`), `LayerId::new(`, `.layer_painter(` (outside `z_order.rs` and
  `focus.rs`), and `.move_to_top(`.
- **Runtime enforcer (`z_order::enforce`):** Runs every frame at the end of
  `MareaderApp::ui` to shield higher tiers against lower-tier clicks and repair
  any within-bucket inversion without requesting repaints once settled.
- **Automated tests:**
  - Pure `egui::Context` unit & integration tests in `crates/ui-egui/src/z_order.rs`
    and `crates/ui-egui/tests/z_order_enforce.rs` verify tier monotonicity,
    `PageCard < FloatingTool < Sidebar < Popover`, click-shielding, 3-tier
    inversion repair, sibling focus, closed-layer non-revival, `Order::Middle`
    chrome ordering, and pre-modal vs modal-hosted popups.
  - Full-app headless tests in `crates/ui-egui/tests/z_order.rs` verify split
    pane `page-bar` vs `dict-window` overlap and hit-testing, overlay
    `sidebar-float` over `dict-window`, `titlebar` popovers, Settings `Dialog`
    + `ComboBox`, Command Palette expanded list height, `toasts` and
    `toast-island`, and parked dictionary reveal strips under an open modal.
