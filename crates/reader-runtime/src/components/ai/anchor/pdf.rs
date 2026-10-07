//! The PDF's anchor half: a rect's screen box, and selection
//! capture.

use ai_core::gloss::{GlossBox, GlossMark, PageAnchor};
use reader_core::view::ViewMode;

use crate::components::ai::gloss::mark_layer::MARK_RADIUS;
use crate::components::ai::reflow_anchor::union_box;
use crate::components::viewer::page_host::host_id_for_mode;
use crate::pane::dom::PaneDom;
use app_chrome::hooks::dom::range_rects;
use app_state::dom_contract::{HOST_ATTR, HOST_PDF};

use super::{FormatAnchorBridge, captured_mark, selection_start};

/// The PDF's bridge: a host rect plus the mark's page rect.
#[derive(Clone, Copy)]
pub struct PdfAnchorBridge {
    /// The view mode, which decides which host element carries the page.
    pub mode: ViewMode,
    /// The pane whose page hosts the anchor is looked up in.
    pub dom: PaneDom,
}

impl FormatAnchorBridge for PdfAnchorBridge {
    fn screen_box(&self, anchor: &PageAnchor, scale: f64) -> Option<GlossBox> {
        screen_box(anchor, scale, self.mode, self.dom)
    }

    fn capture(&self, scale: f64) -> Option<PageAnchor> {
        capture_selection(scale)
    }
}

/// The 1-based page a host id names, for four id shapes.
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

/// The live viewport box for a page anchor, `None` off-page.
pub fn screen_box(
    anchor: &PageAnchor,
    scale: f64,
    mode: ViewMode,
    dom: PaneDom,
) -> Option<GlossBox> {
    if scale <= 0.0 {
        return None;
    }
    let hr = dom
        .by_id(&host_id_for_mode(mode, anchor.page))?
        .get_bounding_client_rect();
    let h = anchor.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + anchor.rect.x * scale,
        y: hr.top() + anchor.rect.y * scale,
        w: anchor.rect.w * scale,
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// Capture the DOM selection as a page-space anchor.
pub fn capture_selection(scale: f64) -> Option<PageAnchor> {
    if scale <= 0.0 {
        return None;
    }
    let (range, el) = selection_start()?;
    let host = el.closest(&format!("[{HOST_ATTR}]")).ok().flatten()?;
    if host.get_attribute(HOST_ATTR).as_deref() != Some(HOST_PDF) {
        // Another format's host: not a
        // page-space rect.
        return None;
    }
    let page = page_from_host_id(&host.id())?;
    let hr = host.get_bounding_client_rect();
    // One rect walk and union rule for every format.
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

/// The same capture as a mark, the pill's fallback.
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
