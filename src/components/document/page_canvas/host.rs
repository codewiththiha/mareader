//! Direct DOM manipulation of a `.pdf-page` host element.
//!
//! Split out of the `PageCanvas` component: these are plain functions over a
//! `web_sys::Element` with no reactivity of their own. Keeping them apart from
//! the component makes it obvious that the component decides WHEN the host
//! changes, while this module knows HOW.

use wasm_bindgen::JsCast;

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
pub(super) fn stretch_host(
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
    // Read-ahead pages off screen do not need a snapshot mask — it is a
    // full-size RGBA copy nobody sees.
    if let Some(win) = web_sys::window() {
        let vh = win
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let rect = host_el.get_bounding_client_rect();
        if vh > 0.0 && (rect.bottom() < 0.0 || rect.top() > vh) {
            return;
        }
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
    _ = snap.set_attribute("class", "page-snapshot");
    if let Some(dst) = snap.dyn_ref::<web_sys::HtmlCanvasElement>() {
        dst.set_width(src.width());
        dst.set_height(src.height());
        if let Ok(Some(ctx)) = dst.get_context("2d")
            && let Some(ctx2d) = ctx.dyn_ref::<web_sys::CanvasRenderingContext2d>()
        {
            _ = ctx2d.draw_image_with_html_canvas_element(&src, 0.0, 0.0);
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
                _ = host_node.insert_before(snap_node, Some(&next));
            }
            None => {
                _ = host_node.append_child(snap_node);
            }
        }
    }
}

/// Remove every `.page-snapshot` overlay from a `.pdf-page` host. Iterates
/// backwards because `query_selector_all` returns a live NodeList: removing a
/// node shifts later indices, so a forward loop could skip one.
///
/// The backing store is zeroed BEFORE the node is removed. WKWebView (Tauri)
/// does not release a canvas IOSurface on DOM removal alone, so a snapshot
/// dropped after every zoom would otherwise leak a full-page RGBA buffer.
pub(super) fn remove_snapshots(host: &web_sys::Element) {
    if let Ok(stale) = host.query_selector_all(".page-snapshot") {
        let mut i = stale.length();
        while i > 0 {
            i -= 1;
            if let Some(n) = stale.get(i) {
                if let Some(cv) = n.dyn_ref::<web_sys::HtmlCanvasElement>() {
                    cv.set_width(0);
                    cv.set_height(0);
                }
                if let Some(el) = n.dyn_ref::<web_sys::Element>() {
                    el.remove();
                }
            }
        }
    }
}
