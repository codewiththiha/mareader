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
use leptos::task::spawn_local;
use wasm_bindgen::JsCast;

use crate::api::engine;
use crate::core::state::AppState;

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
    let state = use_context::<AppState>();
    let texture = move || {
        state
            .as_ref()
            .map(|s| s.settings.get().texture.as_str().to_string())
            .unwrap_or_else(|| "none".to_string())
    };
    let host_class = move || {
        let t = texture();
        let base = if t == "none" {
            "pdf-page".to_string()
        } else {
            format!("pdf-page texture-{t}")
        };
        if class.is_empty() {
            base
        } else {
            format!("{base} {class}")
        }
    };

    let registered = Rc::new(Cell::new(false));

    // Owned clones for the side-effect closures so the originals stay for view!.
    let cid = canvas_id.clone();
    let cid_effect = canvas_id.clone();
    let hid_effect = host_id.clone();
    on_cleanup(move || engine::unregister_page(&cid));

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
        let s = match render_scale_sig {
            Some(rs) => rs.get(),
            None => scale.get(),
        };
        if s <= 0.0 {
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
        if anim {
            if !has_geo {
                let _ = engine::blit_thumb(&cid_effect, page);
            }
            return;
        }
        let page_no = page;
        let cid = cid_effect.clone();
        let hid = hid_effect.clone();
        let rt = render_text;
        let cb = on_geometry.clone();
        let do_register = registered.clone();
        let geo_async = geo.clone();
        let seq_async = render_seq.clone();

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
            let _ = engine::blit_thumb(&cid, page_no);
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

/// Resize a `.pdf-page` host so its EXISTING bitmap stretches to `new_scale`,
/// optionally masking the canvas with a pixel copy first.
///
/// The canvas' CSS box is 100% of the host, so changing the host's size is all
/// it takes to rescale what is already on screen — instantly, with no render.
/// `--scale-factor` moves with it so the text layer's custom-property math
/// (font sizes, `setLayerDimensions` container sizing) stays aligned; dropping
/// it would recompute the layer at scale 1 and misalign selection.
///
/// `mask` should be true only when a render is about to run: pdf.js reassigns
/// `canvas.width/height` at render start, which wipes the live backing store
/// and shows white until the new frame paints. During a zoom ANIMATION no
/// render happens, so no mask is wanted — the real bitmap must stay visible to
/// be stretched.
fn stretch_host(
    host_id: &str,
    canvas_id: &str,
    last_w: f64,
    last_h: f64,
    last_scale: f64,
    new_scale: f64,
    mask: bool,
) {
    let doc = web_sys::window().and_then(|w| w.document());
    let Some(host_el) = doc.as_ref().and_then(|d| d.get_element_by_id(host_id)) else {
        return;
    };
    let _ = host_el.set_attribute(
        "style",
        &format!(
            "width:{}px;height:{}px;--scale-factor:{}",
            last_w * new_scale / last_scale,
            last_h * new_scale / last_scale,
            new_scale
        ),
    );
    if !mask {
        return;
    }
    let Some(src) = doc
        .as_ref()
        .and_then(|d| d.get_element_by_id(canvas_id))
        .and_then(|el| el.dyn_ref::<web_sys::HtmlCanvasElement>().cloned())
    else {
        return;
    };
    if src.width() == 0 || src.height() == 0 {
        return;
    }
    // REUSE an existing snapshot instead of replacing it: a still-running
    // previous render may already have wiped the live canvas, so a fresh copy
    // would be blank and re-expose the flash. The old snapshot holds a pre-wipe
    // bitmap and stretches with the host; the latest completion removes it.
    let has_snapshot = host_el
        .query_selector_all(".page-snapshot")
        .map(|l| l.length() > 0)
        .unwrap_or(false);
    if has_snapshot {
        return;
    }
    let Some(snap) = doc.as_ref().and_then(|d| d.create_element("canvas").ok()) else {
        return;
    };
    let _ = snap.set_attribute("class", "page-snapshot");
    if let Some(dst) = snap.dyn_ref::<web_sys::HtmlCanvasElement>() {
        dst.set_width(src.width());
        dst.set_height(src.height());
        if let Ok(Some(ctx)) = dst.get_context("2d") {
            if let Some(ctx2d) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>() {
                let _ = ctx2d.draw_image_with_html_canvas_element(&src, 0.0, 0.0);
            }
        }
    }
    // Insert between the canvas and the textLayer. web-sys 0.3 has no Deref
    // chain: Node-only methods (next_sibling, insert_before, append_child) need
    // a Node cast.
    if let (Some(src_node), Some(snap_node), Some(host_node)) = (
        src.dyn_ref::<web_sys::Node>(),
        snap.dyn_ref::<web_sys::Node>(),
        host_el.dyn_ref::<web_sys::Node>(),
    ) {
        match src_node.next_sibling() {
            Some(next) => {
                let _ = host_node.insert_before(snap_node, Some(&next));
            }
            None => {
                let _ = host_node.append_child(snap_node);
            }
        }
    }
}

/// Remove every `.page-snapshot` overlay from a `.pdf-page` host. Iterates
/// backwards because `query_selector_all` returns a live NodeList: removing a
/// node shifts later indices, so a forward loop could skip one.
fn remove_snapshots(host: &web_sys::Element) {
    if let Ok(stale) = host.query_selector_all(".page-snapshot") {
        let mut i = stale.length();
        while i > 0 {
            i -= 1;
            if let Some(n) = stale.get(i) {
                if let Some(el) = n.dyn_ref::<web_sys::Element>() {
                    el.remove();
                }
            }
        }
    }
}
