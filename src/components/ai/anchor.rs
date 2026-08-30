//! Shared page-space anchor watchers: glue a [`PageAnchor`] to the live page
//! host so the selection Info pill and the gloss card both follow scroll/zoom
//! and die when their origin leaves a configurable band of the viewport.
//!
//! The pure data type lives in `pdf_core::gloss::PageAnchor` so state can hold
//! it without depending on the component layer.

use leptos::prelude::*;
use pdf_core::gloss::{GlossBox, GlossMark};
use pdf_core::layout::ViewMode;
use wasm_bindgen::JsCast;

use crate::components::ai::gloss::mark_layer::MARK_RADIUS;
use crate::components::primitives::hooks::dom::by_id;
use crate::components::primitives::hooks::use_viewport::viewport_size;
use crate::components::primitives::hooks::use_window_event::{add_window_capture_listener, use_window_event};

// Single public binding — do not also `use` PageAnchor above or rustc E0252s.
pub use pdf_core::gloss::PageAnchor;

/// The selection "Info" pill lives until its origin fully leaves the viewport.
///
/// `1.0` is deliberate, not an untuned placeholder: the pill is small and
/// passive (it morphs nothing and owns no screen real estate), so it should
/// never vanish while any part of the text it points at is still visible —
/// unlike the gloss card below, which covers content and yields earlier.
pub const MENU_EXIT_FRAC: f64 = 1.0;
/// The expanded gloss card tolerates scroll until its origin passes this
/// fraction of the viewport height (or leaves the top edge).
pub const CARD_EXIT_FRAC: f64 = 0.8;

pub fn host_id_for_mode(page: u32, mode: ViewMode) -> String {
    match mode {
        ViewMode::Single => format!("sp-{page}-pg"),
        ViewMode::Spread => format!("dp-{page}-pg"),
        ViewMode::ScrollHorizontal => format!("hp-{page}-pg"),
        ViewMode::ScrollVertical => format!("cont-{}-pg", page.saturating_sub(1)),
    }
}

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
    let hr = by_id(&host_id_for_mode(anchor.page, mode))?.get_bounding_client_rect();
    let h = anchor.rect.h * scale;
    Some(GlossBox {
        x: hr.left() + anchor.rect.x * scale,
        y: hr.top() + anchor.rect.y * scale,
        w: anchor.rect.w * scale,
        h,
        r: MARK_RADIUS.min(h / 2.0),
    })
}

/// Capture the current DOM selection as a page-space anchor.
///
/// The page number comes from the actual `.pdf-page` host under the selection,
/// not from the reader's current-page signal. In the virtualized continuous
/// reader those can temporarily diverge, and anchoring to the signal can point
/// at an unmounted page host — which makes the floating Info pill vanish even
/// though the selection itself is valid and visible.
pub fn capture_selection(scale: f64) -> Option<PageAnchor> {
    if scale <= 0.0 {
        return None;
    }
    let sel = web_sys::window()?.get_selection().ok()??;
    if sel.is_collapsed() || sel.range_count() == 0 {
        return None;
    }
    let range = sel.get_range_at(0).ok()?;
    let node = range.start_container().ok()?;
    let el = node
        .parent_element()
        .or_else(|| node.dyn_into::<web_sys::Element>().ok())?;
    let host = el.closest(".pdf-page").ok().flatten()?;
    let page = page_from_host_id(&host.id())?;
    let hr = host.get_bounding_client_rect();
    let rects = range.get_client_rects()?;
    let (mut l, mut t, mut r, mut b) = (
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    );
    let mut found = false;
    for i in 0..rects.length() {
        let Some(rc) = rects.get(i) else {
            continue;
        };
        if rc.width() <= 0.0 || rc.height() <= 0.0 {
            continue;
        }
        found = true;
        l = l.min(rc.left());
        t = t.min(rc.top());
        r = r.max(rc.right());
        b = b.max(rc.bottom());
    }
    if !found {
        return None;
    }
    Some(PageAnchor {
        page,
        rect: GlossBox {
            x: (l - hr.left()) / scale,
            y: (t - hr.top()) / scale,
            w: ((r - l) / scale).max(1.0),
            h: ((b - t) / scale).max(1.0),
            r: 0.0,
        },
    })
}

pub fn capture_selection_mark(scale: f64, word: String, context: String) -> Option<GlossMark> {
    let a = capture_selection(scale)?;
    Some(GlossMark {
        id: format!("g{}-{}", a.page, js_sys::Date::now() as u64),
        page: a.page,
        word,
        context,
        rect: a.rect,
    })
}

#[derive(Clone, Copy)]
pub struct AnchorWatch {
    /// Live viewport-space box of the anchor (None = page not mounted).
    pub screen: RwSignal<Option<GlossBox>>,
    /// Origin left the allowed band: above the viewport top, or below
    /// `exit_frac` of the viewport height (or the page unmounted).
    pub exited: RwSignal<bool>,
    /// Synchronous re-derive (reads the DOM now). Call before using `screen`
    /// inside the same tick that the mark changed.
    pub refresh: Callback<()>,
}

/// Reusable "glued to the page, dies when the origin leaves" behaviour.
///
/// The screen box is re-derived whenever scroll / zoom / view mode / page /
/// container size change (plus a capture-phase scroll listener so *any*
/// scroller is caught, and window resize). `exit_frac` is the fraction of the
/// viewport height the origin may reach before `exited` flips: `1.0` means
/// "fully out of the viewport", `0.8` means "past 80% of the height".
pub fn watch_page_anchor(
    anchor: Signal<Option<PageAnchor>>,
    scale: Signal<f64>,
    mode: Signal<ViewMode>,
    scroll_top: Signal<f64>,
    page: Signal<u32>,
    exit_frac: f64,
) -> AnchorWatch {
    let screen = RwSignal::new(None::<GlossBox>);
    let exited = RwSignal::new(false);
    let tick = RwSignal::new(0u32);

    let refresh = Callback::new(move |_| {
        let a = anchor.get_untracked();
        let s = scale.get_untracked();
        let b = a
            .as_ref()
            .and_then(|a| screen_box(a, s, mode.get_untracked()));
        if screen.get_untracked() != b {
            screen.set(b);
        }
        let (_, vh) = viewport_size();
        let out = match b {
            None => true,
            Some(b) => (b.y + b.h) < 0.0 || b.y > vh * exit_frac,
        };
        if exited.get_untracked() != out {
            exited.set(out);
        }
    });

    Effect::new(move |_| {
        let _ = anchor.get();
        let _ = scale.get();
        let _ = mode.get();
        let _ = scroll_top.get();
        let _ = page.get();
        let _ = tick.get();
        refresh.run(());
    });

    add_window_capture_listener("scroll", move |_| tick.update(|n| *n += 1));
    use_window_event("resize", move |_| tick.update(|n| *n += 1));

    AnchorWatch {
        screen,
        exited,
        refresh,
    }
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
