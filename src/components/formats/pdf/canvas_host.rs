//! Direct DOM manipulation of a `.pdf-page` host element.
//!
//! Split out of the `PdfPageCanvas` component: these are plain functions over a
//! `web_sys::Element` with no reactivity of their own. Keeping them apart from
//! the component makes it obvious that the component decides WHEN the host
//! changes, while this module knows HOW.

use wasm_bindgen::JsCast;

use pdf_core::pixel_grid::snap_px;

use crate::dom_contract::PAGE_SNAPSHOT_CLASS;

/// The last successfully rendered geometry of a host: the size and scale a
/// stretch rescales FROM. Deliberately the RAW (unsnapped) values — the
/// stretch ratio must not drift across successive steps, while the size it
/// writes is snapped (see the note below).
#[derive(Clone, Copy)]
pub(super) struct LastGeo {
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

/// Resize a `.pdf-page` host so its EXISTING bitmap stretches to `new_scale`,
/// without allocating a second raster.
///
/// The canvas' CSS box is 100% of the host, so changing the host's size is all
/// it takes to rescale what is already on screen — instantly, with no render.
/// `--scale-factor` moves with it so the text layer's custom-property math
/// (font sizes, `setLayerDimensions` container sizing) stays aligned; dropping
/// it would recompute the layer at scale 1 and misalign selection.
///
/// The renderer swaps a completed offscreen raster atomically, so stretching
/// never needs a second full-size snapshot of the displayed pixels.
///
/// The stretched size is snapped to the device-pixel grid: the raw product
/// `size × scale` is fractional at almost every zoom step, and a page whose
/// layer rect rounds one way while its neighbour's rounds the other shows the
/// backdrop through the joint as a hairline (see [`pdf_core::pixel_grid`]).
/// The scale ratio itself stays raw, so repeated stretches cannot drift.
pub(super) fn stretch_host(
    host_id: &str,
    last: LastGeo,
    new_scale: f64,
) {
    let Some(host_el) = app_chrome::hooks::dom::by_id(host_id) else {
        return;
    };
    let _ = host_el.set_attribute(
        "style",
        &format!(
            "width:{}px;height:{}px;--scale-factor:{}",
            snap_px(last.w * new_scale / last.scale),
            snap_px(last.h * new_scale / last.scale),
            new_scale
        ),
    );
}

/// Remove every `.page-snapshot` overlay from a `.pdf-page` host. Iterates
/// backwards because `query_selector_all` returns a live NodeList: removing a
/// node shifts later indices, so a forward loop could skip one.
///
/// The backing store is zeroed BEFORE the node is removed. WKWebView (Tauri)
/// does not release a canvas IOSurface on DOM removal alone, so a snapshot
/// dropped after every zoom would otherwise leak a full-page RGBA buffer.
pub(super) fn remove_snapshots(host: &web_sys::Element) {
    if let Ok(stale) = host.query_selector_all(&format!(".{PAGE_SNAPSHOT_CLASS}")) {
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
