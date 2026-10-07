//! DOM manipulation of a `.pdf-page` host: the component decides WHEN,
//! this knows HOW.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use pdf_core::pixel_grid::snap_px;

use app_state::dom_contract::PAGE_SNAPSHOT_CLASS;

/// The host's last rendered geometry: what a stretch rescales FROM.
#[derive(Clone, Copy)]
pub(super) struct LastGeo {
    pub w: f64,
    pub h: f64,
    pub scale: f64,
}

/// Resize the host so its existing bitmap stretches to `new_scale`.
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

/// The host element by its own reference, never by id.
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
    /// A render landing in a zoom state it was not issued for.
    Stale,
}

/// Judge a render completion against the zoom state it lands in.
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
        // Mid-transaction only the first-paint stop-gap may land.
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

/// Remove every `.page-snapshot` overlay from the host.
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
        // Issued at 1.0, lands while the zoom to 1.25 is open.
        assert_eq!(
            judge_completion(true, false, true, 1.0, 1.0),
            Completion::Stale
        );
    }

    #[test]
    fn a_render_from_before_the_zoom_cannot_land_after_the_commit() {
        // Lands after release, before the commit's own render.
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
