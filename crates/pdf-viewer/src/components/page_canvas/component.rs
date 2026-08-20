//! The shared page host: a `.pdf-page` div containing a `<canvas>` and a
//! `.textLayer` div, driven by the JS engine. Used by BOTH view modes.
//!
//! Contract:
//!  - ids are Rust-chosen unique strings; the engine resolves elements by id.
//!  - renders (page, scale) via engine.renderPage / updatePage; reports the
//!    rendered CSS-px size through `on_geometry`.
//!  - registers on first render, unregisters (cancels) when disposed.
//!
//! TWO EFFECTS, deliberately separated (see `effects::fit`):
//!
//!   * the STRETCH effect follows the `scale` prop — which callers wire to
//!     `viewer.display_scale`. It only ever resizes the host so the bitmap we
//!     already have CSS-stretches to the new layout size. It runs every frame
//!     of a zoom and never triggers a render.
//!   * the RENDER effect follows `viewer.render_scale` and is suspended while
//!     `viewer.zoom_animating` is true. It produces the one crisp rasterisation
//!     at the end of a gesture.
//!
//! Keeping these in ONE effect was the ghost/double-image bug: a scale change
//! resized the host and kicked off a render in the same run, so every
//! intermediate frame of a zoom cancelled and restarted a rasterisation, and
//! the half-drawn results were what flashed.

use std::cell::Cell;
use std::rc::Rc;

use leptos::prelude::*;

use super::host::{remove_snapshots, stretch_host};
use leptos::task::spawn_local;

use pdf_engine::api as engine;
use pdf_core::appearance::TextureMode;
use crate::state::ViewerState;

#[component]
pub fn PageCanvas(
    /// 1-based page number this host renders.
    page: u32,
    /// Render scale (CSS px per PDF unit).
    scale: ReadSignal<f64>,
    /// Unique id for the <canvas> element.
    canvas_id: String,
    /// Unique id for the .pdf-page host element.
    host_id: String,
    /// Whether to build a text layer for selection + search highlights.
    render_text: bool,
    /// Extra classes (e.g. absolute positioning in the continuous layout).
    #[prop(default = String::new())]
    class: String,
    /// Called with (page, width, height) CSS px after each successful render.
    #[prop(optional)]
    on_geometry: Option<Callback<(u32, f64, f64)>>,
) -> impl IntoView {
    let state = use_context::<ViewerState>();
    // Texture comes from the app shell via context (derived from settings);
    // a Memo so only a real texture change rebuilds the host class.
    let texture = {
        let ctx = use_context::<crate::state::TextureSignal>();
        Memo::new(move |_| ctx.map(|t| t.get()).unwrap_or(TextureMode::None))
    };
    let host_class = move || {
        let t = texture.get();
        let base = if t == TextureMode::None {
            "pdf-page".to_string()
        } else {
            format!("pdf-page texture-{}", t.as_str())
        };
        if class.is_empty() {
            base
        } else {
            format!("{base} {class}")
        }
    };

    let registered = Rc::new(Cell::new(false));
    // PAINTED FLAG (Fix C). True after a successful render; false after a
    // cancelled/error render (which leaves the canvas wiped by pdf.js's
    // `canvas.width = ...` on render start). The no-op fast path below
    // requires `painted == true` so a wiped canvas always re-renders even
    // at unchanged scale — otherwise a page whose render was cancelled
    // mid-flight during a sidebar slide (remount race) would sit blank
    // until a scroll re-triggered the effect.
    let painted = Rc::new(Cell::new(false));

    // Owned clones for the side-effect closures so the originals stay for view!.
    let cid = canvas_id.clone();
    let cid_effect = canvas_id.clone();
    let hid_effect = host_id.clone();
    on_cleanup(move || engine::unregister_page(&cid));

    // Register after this view is flushed to the DOM. The render effect can
    // otherwise call register_page before getElementById sees the canvas.
    let cid_boot = canvas_id.clone();
    let hid_boot = host_id.clone();
    let registered_boot = registered.clone();
    queue_microtask(move || {
        if !registered_boot.get() {
            engine::register_page(page, &cid_boot, Some(&hid_boot));
            registered_boot.set(true);
        }
    });

    // Last successfully rendered host geometry (CSS-px width, height, scale).
    // Guard 1 (stretch-resize) reads it to size the host before a re-render
    // lands; only written on a successful render.
    let geo = StoredValue::new_local((0.0f64, 0.0f64, 0.0f64));
    // Monotonic render generation: only the latest render may apply geometry
    // or fire the callback, so a stale in-flight render (one whose cancel the
    // engine missed) cannot overwrite a newer host size — which would re-add
    // the size jump this unit removes.
    let render_seq = StoredValue::new_local(0u32);

    // `render_scale` / `zoom_animating` come from the app state; the `scale`
    // prop is the DISPLAY scale. Outside a provider (tests/isolated mounts) we
    // fall back to the prop for both, which restores the old single-scale
    // behaviour rather than rendering nothing.
    let render_scale_sig = state.as_ref().map(|s| s.viewer.render_scale);
    let zoom_anim_sig = state.as_ref().map(|s| s.viewer.zoom_animating);

    // --- Stretch effect ------------------------------------------------------
    // Follows `display_scale`. Pure CSS: resize the host so the EXISTING bitmap
    // scales with the layout, and mask the moment a render is going to wipe it.
    // Never renders — that is the whole point of the split.
    let hid_stretch = host_id.clone();
    let cid_stretch = canvas_id.clone();
    Effect::new(move || {
        let s = scale.get();
        if s <= 0.0 {
            return;
        }
        let (lw, lh, ls) = geo.get_value();
        // Nothing rendered yet => nothing to stretch; the render effect owns
        // the first paint.
        if lw <= 0.0 || lh <= 0.0 || ls <= 0.0 || (ls - s).abs() <= 1e-9 {
            return;
        }
        stretch_host(&hid_stretch, &cid_stretch, lw, lh, ls, s, false);
    });

    // --- Render effect -------------------------------------------------------
    Effect::new(move || {
        // Read every dependency unconditionally: a Leptos effect only
        // subscribes to what it READS during a run, so a conditional read would
        // silently drop the subscription the first time the branch was skipped.
        let anim = zoom_anim_sig.map(|z| z.get()).unwrap_or(false);
        let s_render = match render_scale_sig {
            Some(rs) => rs.get(),
            None => scale.get(),
        };
        if s_render <= 0.0 {
            return;
        }
        let (gw, gh, gs) = geo.get_value();
        let has_geo = gw > 0.0 && gh > 0.0 && gs > 0.0;
        // A zoom or a sidebar slide is in flight: stay out of the way entirely.
        //
        // With a bitmap, rendering here would relayout mid-animation — the
        // teleport/flicker this whole design removes. WITHOUT one (a page that
        // just scrolled into the window, which happens constantly during a
        // slide because the shrinking scale fits more pages on screen), the
        // render would be at a scale that is already obsolete: it resolves,
        // reports geometry, and is immediately superseded by the commit pass.
        // Measured on one sidebar toggle, that was 3 of 11 renders — wasted
        // work whose only visible effect is a page popping in at the wrong
        // size. The thumbnail underlay below covers the gap, and the commit
        // pass (~200ms later) renders it once, correctly.
        //
        // FIX A (cold-cache first paint). If the page has NO bitmap yet
        // (`!has_geo`) AND the thumbnail cache misses (`blit_thumb` returns
        // false — e.g. the sidebar was never opened this session), the page
        // would sit as an EMPTY TRANSPARENT CANVAS for the entire 300ms slide
        // + 120ms commit debounce. That is why "the one in view just
        // disappeared" and the stretch animation was invisible: the node
        // that was supposed to stretch had no bitmap to stretch. Fix: fall
        // through to the masked-render path below, but at the DISPLAY scale
        // (read untracked so we don't subscribe to per-frame display_scale
        // changes — the effect must NOT re-run every frame of the slide).
        // `on_geometry` already ignores writes while `zoom_animating`, and
        // `commit_scale` re-renders crisply at `render_scale` when the
        // gesture settles. The stretch effect keeps tracking `display_scale`
        // afterwards, so the first-paint bitmap CSS-stretches with the slide.
        if anim {
            if has_geo {
                return; // stretch effect owns it
            }
            if engine::blit_thumb(&cid_effect, page) {
                return; // cached thumbnail is fine
            }
            // FALLTHROUGH: first render at the DISPLAY scale so the slide
            // has pixels. Read untracked to avoid subscribing to per-frame
            // display_scale changes (which would re-run this effect every
            // frame of the slide — a render storm).
            // The `s` used below is `s_render` by default; override it for
            // this fallthrough path.
        }
        // Use the display scale for the cold-cache first paint during an
        // animation; otherwise use the render scale (the crisp target).
        let s = if anim { scale.get_untracked() } else { s_render };
        if s <= 0.0 {
            return;
        }
        // NO-OP FAST PATH (Fix C). If the page has already been rendered at
        // THIS scale AND the canvas still has its bitmap (`painted == true`),
        // re-rendering would only WIPE the live canvas (pdf.js reassigns
        // `canvas.width/height` on render start) without producing a different
        // bitmap. Because `(gs - s).abs() <= 1e-9`, the `stretch_host(...,
        // mask=true)` guard below is skipped too — so no `.page-snapshot`
        // overlay is created to mask the wipe — and the user sees the canvas
        // disappear until a scroll re-renders it. Bail out — but ONLY if
        // `painted == true`. A wiped canvas (cancelled render) must re-render.
        if has_geo && painted.get() && (gs - s).abs() <= 1e-9 {
            return;
        }
        let page_no = page;
        let cid = cid_effect.clone();
        let hid = hid_effect.clone();
        let rt = render_text;
        let cb = on_geometry;
        let do_register = registered.clone();
        let geo_async = geo;
        let seq_async = render_seq;
        let painted_async = painted.clone();

        // This effect run owns the next generation; older completions are stale.
        let my_seq = seq_async.get_value() + 1;
        seq_async.set_value(my_seq);

        // Flicker guards, for renders the stretch effect did NOT precede —
        // e.g. the search nudge, or a fit refit that lands straight on
        // render_scale. Sizes the host to the incoming scale and masks the
        // canvas before pdf.js wipes it. When a zoom gesture just ended the
        // stretch effect has already done this and the mask is reused.
        let (lw, lh, ls) = geo.get_value();
        if lw > 0.0 && lh > 0.0 && ls > 0.0 && (ls - s).abs() > 1e-9 {
            stretch_host(&hid, &cid, lw, lh, ls, s, true);
        }

        // First paint for this host: drop in the sidebar's cached thumbnail,
        // upscaled, so the card reads as a blurry version of the right page
        // instead of flashing white until the render lands. No-op when nothing
        // is cached, and immediately overwritten by the real bitmap.
        if !(lw > 0.0 && lh > 0.0) {
            engine::blit_thumb(&cid, page_no);
        }

        spawn_local(async move {
            // A render can outlive its component: `<For>` unmounts a page that
            // scrolled out of the window while its rasterisation is still in
            // flight. Once the reactive owner is disposed, TOUCHING a
            // StoredValue panics, so every post-await access goes through
            // `try_get_value`. Nothing needs doing in that case — the element
            // is gone and the engine already dropped the registration.
            if !do_register.get() {
                engine::register_page(page_no, &cid, Some(&hid));
                do_register.set(true);
            }
            match engine::render_page(&cid, s, rt).await {
                Ok(r) => {
                    // Unmounted mid-render, or a newer scale change superseded
                    // this one: leave the geometry + mask to the newer task
                    // (stale hosts caused the size jump this unit removes).
                    if seq_async.try_get_value() != Some(my_seq) {
                        return;
                    }
                    // Successful render: the canvas now has a bitmap.
                    painted_async.set(true);
                    if let Some(host) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&hid))
                    {
                        // Note: cannot use host.style() (tachys ElementExt::style shadows
                        // web_sys' inherent method); set the inline style attribute directly.
                        // The engine also sets `--scale-factor` inline on the host during
                        // render; this FULL attribute replace must carry it forward or the
                        // text layer's custom-property math (font-size + setLayerDimensions
                        // container sizing) recomputes at scale 1 and misaligns selection.
                        let _ = host.set_attribute(
                            "style",
                            &format!(
                                "width:{}px;height:{}px;--scale-factor:{}",
                                r.width, r.height, s
                            ),
                        );
                        // New bitmap is live — drop any mask in the same flush.
                        remove_snapshots(&host);
                    }
                    geo_async.try_set_value((r.width, r.height, s));
                    if let Some(cb) = cb {
                        cb.run((page_no, r.width, r.height));
                    }
                }
                Err(e) => {
                    // A stale or orphaned completion must not touch the host
                    // or the mask.
                    if seq_async.try_get_value() != Some(my_seq) {
                        return;
                    }
                    // Cancelled / transient errors are logged, not fatal; never
                    // leave a stale mask behind (a cancelled render also leaves
                    // the canvas wiped, so the next scale change re-marks).
                    // Mark the canvas as NOT painted so the no-op fast path
                    // does not skip the re-render (Fix C).
                    painted_async.set(false);
                    if let Some(host) = web_sys::window()
                        .and_then(|w| w.document())
                        .and_then(|d| d.get_element_by_id(&hid))
                    {
                        remove_snapshots(&host);
                    }
                    web_sys::console::log_1(
                        &format!("[page_canvas] render page {page_no}: {e}").into(),
                    );
                }
            }
        });
    });

    view! {
        <div id=host_id class=host_class>
            <canvas id=canvas_id />
            // Placeholder text layer. The engine REPLACES this node on each
            // text render: it builds the spans in a detached `.textLayer` and
            // swaps it in atomically, so a superseded render's late-arriving
            // spans can never land on top of the current ones (that overlap was
            // the doubled text visible when selecting). Leptos does not own the
            // node's contents, so the swap is safe — but keep the class name
            // and position (immediately after the canvas) in sync with
            // `renderPageInternal` in public/pdfEngine.js.
            <div class="textLayer" aria-hidden="true"></div>
        </div>
    }
}
