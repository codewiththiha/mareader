//! Direct DOM manipulation of a `.pdf-page` host element.
//!
//! Split out of the `PdfPageCanvas` component: these are plain functions over a
//! `web_sys::Element` with no reactivity of their own. Keeping them apart from
//! the component makes it obvious that the component decides WHEN the host
//! changes, while this module knows HOW.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::pixel_grid::snap_px;

use app_state::dom_contract::PAGE_SNAPSHOT_CLASS;

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

/// Resize a `.pdf-page` host so its EXISTING bitmap stretches to `new_scale`.
///
/// The canvas' CSS box is 100% of the host, so changing the host's size is all
/// it takes to rescale what is already on screen — instantly, with no render.
/// `--scale-factor` moves with it so the text layer's custom-property math
/// (font sizes, `setLayerDimensions` container sizing) stays aligned; dropping
/// it would recompute the layer at scale 1 and misalign selection.
///
/// Nothing is masked here, and nothing needs to be: a page render draws into
/// a scratch and replaces the visible bitmap in one blit when it lands
/// (`renderPageNow` in public/engine/renderer.ts), so the stretched bitmap
/// stays on screen for the whole raster. (This function used to be able to
/// stack a `.page-snapshot` copy over the canvas; every caller declined it,
/// and the only producer of those covers left is the appearance scrub.)
///
/// The stretched size is snapped to the device-pixel grid: the raw product
/// `size × scale` is fractional at almost every zoom step, and a page whose
/// layer rect rounds one way while its neighbour's rounds the other shows the
/// backdrop through the joint as a hairline (see [`pdf_core::pixel_grid`]).
/// The scale ratio itself stays raw, so repeated stretches cannot drift.
pub(super) fn stretch_host(host: NodeRef<html::Div>, last: LastGeo, new_scale: f64) {
    let Some(host_el) = host_element(host) else {
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

/// The page host element the component rendered, by its own reference —
/// never by id, which a second pane's page host carries too. `None` before
/// the mount and once the component's owner is gone (a render landing after
/// the page was virtualized away), where there is nothing left to touch.
pub(super) fn host_element(host: NodeRef<html::Div>) -> Option<web_sys::Element> {
    host.try_get_untracked()
        .flatten()
        .map(web_sys::Element::from)
}

/// What a finished page render may do to its host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Completion {
    /// The host's current render: size the host, record the geometry,
    /// report it to the strip.
    Apply,
    /// A newer render was issued for this host; that one owns it.
    Superseded,
    /// This host's latest render, landing in a zoom state it was not issued
    /// for: mid-transaction (a zoom, or a container follow holding its
    /// commit), or after the commit moved to another scale. Its bitmap is
    /// already on the canvas (the blit happened engine-side) and is recorded
    /// as the stretch base, but the size at its rendered scale must not reach
    /// the host — it would snap the host back under the current layout — nor
    /// the strip as a measurement.
    Stale,
}

/// Judge a render completion against the zoom state it lands in.
///
/// `render_seq` alone cannot: it only moves when the render effect ISSUES a
/// render, and the effect issues nothing while a transaction is in flight —
/// so a render started before a zoom still owns the current sequence number
/// when it lands mid-zoom or just after the commit.
///
/// * `latest` — this is the host's most recently issued render.
/// * `issued_mid_zoom` — the cold first-paint fallthrough, rendered at the
///   DISPLAY scale while a transaction ran (the page had no pixels at all).
/// * `zooming_now` / `committed_now` — the zoom state at landing.
/// * `rendered_at` — the scale this render rasterised at.
pub(super) fn judge_completion(
    latest: bool,
    issued_mid_zoom: bool,
    zooming_now: bool,
    committed_now: f64,
    rendered_at: f64,
) -> Completion {
    if !latest {
        return Completion::Superseded;
    }
    let current = if zooming_now {
        // Mid-transaction only the first-paint stop-gap may land; a render
        // for the committed scale the transaction is leaving may not.
        issued_mid_zoom
    } else {
        (committed_now - rendered_at).abs() <= 1e-9
    };
    if current {
        Completion::Apply
    } else {
        Completion::Stale
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

#[cfg(test)]
mod completion_tests {
    use super::*;

    #[test]
    fn the_current_render_applies() {
        assert_eq!(
            judge_completion(true, false, false, 1.5, 1.5),
            Completion::Apply
        );
    }

    #[test]
    fn a_newer_render_owns_the_host() {
        assert_eq!(
            judge_completion(false, false, false, 1.5, 1.5),
            Completion::Superseded
        );
        assert_eq!(
            judge_completion(false, true, true, 1.5, 1.5),
            Completion::Superseded
        );
    }

    #[test]
    fn a_render_from_before_the_zoom_cannot_land_mid_transaction() {
        // Issued at the old committed 1.0, lands while the zoom to 1.25 is
        // still open: its geometry would snap the host back to 1.0.
        assert_eq!(
            judge_completion(true, false, true, 1.0, 1.0),
            Completion::Stale
        );
    }

    #[test]
    fn a_render_from_before_the_zoom_cannot_land_after_the_commit() {
        // Lands after the transaction released but before the commit's own
        // render was issued: still the latest, but committed moved on, so
        // its size must not reach the host or the strip's measurements.
        assert_eq!(
            judge_completion(true, false, false, 1.25, 1.0),
            Completion::Stale
        );
    }

    #[test]
    fn the_first_paint_stop_gap_lands_only_while_its_transaction_runs() {
        assert_eq!(
            judge_completion(true, true, true, 1.0, 1.1),
            Completion::Apply
        );
        // After the commit it is an old-scale measurement like any other.
        assert_eq!(
            judge_completion(true, true, false, 1.25, 1.1),
            Completion::Stale
        );
        // ...unless it happened to rasterise at exactly the committed scale.
        assert_eq!(
            judge_completion(true, true, false, 1.25, 1.25),
            Completion::Apply
        );
    }
}
