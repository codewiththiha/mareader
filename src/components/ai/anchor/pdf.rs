//! The PDF's half of the anchor contract: where a page-space rect is on screen
//! right now, and how a live DOM selection becomes one.
//!
//! A PDF page is fixed pixels, so both answers are arithmetic over a host
//! element's rect — no re-projection, and nothing to invalidate beyond scroll
//! and zoom. The reflowable counterpart is
//! [`crate::components::ai::reflow_anchor`], reached through
//! [`super::ReflowAnchorBridge`].

use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};
use reader_core::view::ViewMode;

use app_chrome::hooks::dom::{by_id, range_rects};
use crate::components::ai::gloss::mark_layer::MARK_RADIUS;
use crate::components::ai::reflow_anchor::union_box;
use crate::components::viewer::page_host::host_id_for_mode;
use crate::dom_contract::{HOST_ATTR, HOST_PDF};

use super::{captured_mark, selection_start, FormatAnchorBridge};

/// The PDF's bridge: a page host's rect plus the mark's page-space rect, times
/// the display scale.
#[derive(Clone, Copy)]
pub struct PdfAnchorBridge {
    /// The view mode, which decides which host element carries the page.
    pub mode: ViewMode,
}

impl FormatAnchorBridge for PdfAnchorBridge {
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox> {
        screen_box(anchor, scale, self.mode)
    }

    fn capture(&self, scale: f64) -> Option<PageAnchor> {
        capture_selection(scale)
    }
}

/// The 1-based page a host id names, for the four id shapes
/// [`host_id_for_mode`] builds (`sp-`, `dp-`, `hp-` and the continuous strip's
/// `cont-`, which is a 0-based index). `None` for anything else — a wrapper row,
/// a canvas, an id from another scheme.
fn page_from_host_id(id: &str) -> Option<u32> {
    if let Some(page) = id
        .strip_prefix("sp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    if let Some(page) = id
        .strip_prefix("dp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    if let Some(page) = id
        .strip_prefix("hp-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
    {
        return Some(page);
    }
    id.strip_prefix("cont-")
        .and_then(|rest| rest.strip_suffix("-pg"))
        .and_then(|n| n.parse::<u32>().ok())
        .map(|index| index + 1)
}

/// Live viewport-space box for a page anchor. `None` when the scale is invalid
/// or the host page is not mounted (virtualized away) — which by itself counts
/// as "the anchor left the page".
pub fn screen_box(anchor: &PageAnchor, scale: f64, mode: ViewMode) -> Option<GlossBox> {
    if scale <= 0.0 {
        return None;
    }
    let hr = by_id(&host_id_for_mode(mode, anchor.page))?.get_bounding_client_rect();
    let h = anchor.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + anchor.rect.x * scale,
        y: hr.top() + anchor.rect.y * scale,
        w: anchor.rect.w * scale,
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// Capture the current DOM selection as a page-space anchor, for a format whose
/// identity IS a page-space rect (the PDF).
///
/// The page number comes from the host under the selection, not from the
/// reader's current-page signal. In the virtualized continuous reader those can
/// temporarily diverge, and anchoring to the signal can point at an unmounted
/// page host — which makes the floating Explain pill vanish even though the
/// selection itself is valid and visible.
///
/// The host is found through the `data-reader-host` attribute rather than a
/// `.pdf-page` class, so a format joins this path by tagging its host (see
/// [`crate::components::ai::reflow_anchor`]) and no selector here has to grow a
/// second class. A reflowable document is NOT captured by this function: its
/// anchor needs the block and character offsets the engine's selection tracker
/// reports with the event, so it goes through
/// [`crate::components::ai::reflow_anchor::anchor_of`] instead — which is what
/// `crate::effects::reader::selection_tracking` decides between.
pub fn capture_selection(scale: f64) -> Option<PageAnchor> {
    if scale <= 0.0 {
        return None;
    }
    let (range, el) = selection_start()?;
    let host = el
        .closest(&format!("[{HOST_ATTR}]"))
        .ok()
        .flatten()?;
    if host.get_attribute(HOST_ATTR).as_deref() != Some(HOST_PDF) {
        // Another format's host: its anchor is not a page-space rect, and
        // guessing one here would persist a mark that cannot be projected.
        return None;
    }
    let page = page_from_host_id(&host.id())?;
    let hr = host.get_bounding_client_rect();
    // One rect walk and one union rule for every format, so a stroke can never
    // be a different shape than the card that springs from it.
    let union = union_box(&range_rects(&range))?;
    Some(PageAnchor {
        page,
        rect: GlossBox {
            x: (union.x - hr.left()) / scale,
            y: (union.y - hr.top()) / scale,
            w: (union.w / scale).max(1.0),
            h: (union.h / scale).max(1.0),
            r: 0.0,
        },
    })
}

/// The same capture, as a whole mark — the Explain pill's fallback when the
/// anchor it captured with its selection is gone.
pub fn capture_selection_mark(scale: f64, word: String, context: String) -> Option<GlossMark> {
    Some(captured_mark(word, context, capture_selection(scale)?))
}

#[cfg(test)]
mod tests {
    use super::page_from_host_id;

    #[test]
    fn parses_continuous_host_ids_into_one_based_pages() {
        assert_eq!(page_from_host_id("cont-0-pg"), Some(1));
        assert_eq!(page_from_host_id("cont-11-pg"), Some(12));
    }

    #[test]
    fn parses_single_page_host_ids() {
        assert_eq!(page_from_host_id("sp-1-pg"), Some(1));
        assert_eq!(page_from_host_id("sp-27-pg"), Some(27));
    }

    #[test]
    fn parses_dual_and_horizontal_host_ids() {
        assert_eq!(page_from_host_id("dp-3-pg"), Some(3));
        assert_eq!(page_from_host_id("hp-12-pg"), Some(12));
    }

    #[test]
    fn rejects_unrelated_ids() {
        assert_eq!(page_from_host_id("cont-wrap"), None);
        assert_eq!(page_from_host_id("page-3"), None);
    }
}
