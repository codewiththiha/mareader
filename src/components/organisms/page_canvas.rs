//! The shared page host: a `.pdf-page` div containing a `<canvas>` and a
//! `.textLayer` div, driven by the JS engine. Used by BOTH view modes.
//!
//! Contract:
//!  - ids are Rust-chosen unique strings; the engine resolves elements by id.
//!  - renders (page, scale) via engine.renderPage / updatePage; reports the
//!    rendered CSS-px size through `on_geometry`.
//!  - registers on first render, unregisters (cancels) when disposed.

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

    Effect::new(move || {
        let s = scale.get();
        if s <= 0.0 {
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

        // --- Flicker guard 1: immediate stretch-resize ---------------------
        // Resize the .pdf-page host the instant the render scale changes: the
        // OLD bitmap (canvas CSS box = 100% of the host) stretches for the
        // render's duration instead of the page snapping when the render
        // resolves — a brief blur, not a flash. `--scale-factor` moves with it
        // so the text layer's custom-property math stays aligned.
        let (lw, lh, ls) = geo.get_value();
        if lw > 0.0 && lh > 0.0 && ls > 0.0 && (ls - s).abs() > 1e-9 {
            let doc = web_sys::window().and_then(|w| w.document());
            let host_el = doc.as_ref().and_then(|d| d.get_element_by_id(&hid));
            let src = doc
                .as_ref()
                .and_then(|d| d.get_element_by_id(&cid))
                .and_then(|el| el.dyn_ref::<web_sys::HtmlCanvasElement>().cloned());
            if let (Some(host_el), Some(src)) = (host_el, src) {
                let _ = host_el.set_attribute(
                    "style",
                    &format!(
                        "width:{}px;height:{}px;--scale-factor:{}",
                        lw * s / ls,
                        lh * s / ls,
                        s
                    ),
                );

                // --- Flicker guard 2: bitmap snapshot -----------------------
                // pdf.js reassigns canvas.width/height at render start, wiping
                // the live backing store -> a white flash until the new frame
                // paints. A pixel copy of the current bitmap, inserted between
                // the canvas and the textLayer, masks the in-flight render and
                // is removed the moment the new frame resolves.
                if src.width() > 0 && src.height() > 0 {
                    // REUSE an existing snapshot instead of replacing it: a
                    // still-running previous render may already have wiped the
                    // live canvas, so a fresh copy would be blank and re-expose
                    // the flash. The old snapshot holds a pre-wipe bitmap and
                    // stretches with the host; the latest completion removes it.
                    let has_snapshot = host_el
                        .query_selector_all(".page-snapshot")
                        .map(|l| l.length() > 0)
                        .unwrap_or(false);
                    if !has_snapshot {
                        if let Some(snap) = doc
                            .as_ref()
                            .and_then(|d| d.create_element("canvas").ok())
                        {
                            let _ = snap.set_attribute("class", "page-snapshot");
                            if let Some(dst) = snap.dyn_ref::<web_sys::HtmlCanvasElement>() {
                                dst.set_width(src.width());
                                dst.set_height(src.height());
                                if let Ok(Some(ctx)) = dst.get_context("2d") {
                                    if let Some(ctx2d) =
                                        ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>()
                                    {
                                        let _ = ctx2d
                                            .draw_image_with_html_canvas_element(&src, 0.0, 0.0);
                                    }
                                }
                            }
                            // Insert between the canvas and the textLayer. web-sys
                            // 0.3 has no Deref chain: Node-only methods (next_sibling,
                            // insert_before, append_child) need a Node cast.
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
                    }
                }
            }
        }

        spawn_local(async move {
            if !do_register.get() {
                engine::register_page(page_no, &cid, Some(&hid));
                do_register.set(true);
            }
            match engine::render_page(&cid, s, rt).await {
                Ok(r) => {
                    // A newer scale change superseded this render: leave the
                    // geometry + mask to the newer task (stale hosts caused the
                    // size jump this unit removes).
                    if seq_async.get_value() != my_seq {
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
                    geo_async.set_value((r.width, r.height, s));
                    if let Some(cb) = cb {
                        cb.run((page_no, r.width, r.height));
                    }
                }
                Err(e) => {
                    // A stale completion must not touch the host or the mask.
                    if seq_async.get_value() != my_seq {
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
            <div class="textLayer" aria-hidden="true"></div>
        </div>
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
