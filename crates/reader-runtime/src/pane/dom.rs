//! The pane's own place in the DOM: its root element and the box the host
//! handed it.
//!
//! Every element a pane renders lives under ONE root (the content wrapper
//! `pane_content` builds), and every lookup a pane makes for its own
//! elements — the strips, the page hosts, the block rows, the scrollers —
//! runs INSIDE that root. The ids the elements carry (`page-list`, `sp-3`,
//! a block row's id) stay what they were, because styles, the engine's
//! canvas registry and the browser suite address them; what changed is that
//! a pane can no longer find another pane's element by asking the whole
//! document. Two panes showing the same mode carry the same ids, and each
//! one's lookups answer with its own.
//!
//! The bounds are the host's measurement of the pane's box (`mount` hands
//! the first, `resize` every later one). The pane sizes its root from them
//! and seeds its first fit from them, so its geometry is the box it was
//! given rather than something re-derived from the window.

use leptos::html;
use leptos::prelude::*;

use crate::host::model::PaneBounds;

/// Copy handle onto the pane's root element and its host-given box. Created
/// with the pane's reader state, in the pane's owner: both die with it.
#[derive(Clone, Copy)]
pub struct PaneDom {
    root: NodeRef<html::Div>,
    bounds: RwSignal<PaneBounds>,
}

impl Default for PaneDom {
    fn default() -> Self {
        Self {
            root: NodeRef::new(),
            bounds: RwSignal::new(PaneBounds::default()),
        }
    }
}

impl PaneDom {
    /// The node ref the pane's content root binds (`node_ref=`).
    pub fn root_ref(&self) -> NodeRef<html::Div> {
        self.root
    }

    /// The pane's root element, when it is mounted. Untracked, and `try_`:
    /// a frame or a timer armed before the pane's dispose can ask after the
    /// ref is gone, and the answer then is "nothing mounted".
    pub fn root(&self) -> Option<web_sys::Element> {
        self.root
            .try_get_untracked()
            .flatten()
            .map(web_sys::Element::from)
    }

    /// The element with `id` INSIDE this pane — never another pane's twin.
    pub fn by_id(&self, id: &str) -> Option<web_sys::Element> {
        self.select(&id_selector(id))
    }

    /// The first element matching `selector` inside this pane.
    pub fn select(&self, selector: &str) -> Option<web_sys::Element> {
        self.root()?.query_selector(selector).ok().flatten()
    }

    /// The continuous strip's scroller (`#page-list`) inside this pane.
    pub fn page_list(&self) -> Option<web_sys::Element> {
        self.by_id(app_chrome::hooks::dom::PAGE_LIST_ID)
    }

    /// The horizontal strip's scroller (`#h-page-list`) inside this pane.
    pub fn h_page_list(&self) -> Option<web_sys::Element> {
        self.by_id(app_chrome::hooks::dom::H_PAGE_LIST_ID)
    }

    /// The host handed the pane a box (its first at mount, each later one
    /// at a resize).
    pub(crate) fn set_bounds(&self, bounds: PaneBounds) {
        if self.bounds.try_get_untracked() != Some(bounds) {
            let _ = self.bounds.try_set(bounds);
        }
    }

    /// The pane's box (tracked).
    pub fn bounds(&self) -> PaneBounds {
        self.bounds.try_get().unwrap_or_default()
    }

    /// The box's size, once the host has measured one — `None` before the
    /// first measurement (a pane created ahead of its slot's first layout,
    /// or a frame parked out of layout). Untracked.
    pub fn measured_size(&self) -> Option<(f64, f64)> {
        let bounds = self.bounds.try_get_untracked()?;
        measured(bounds)
    }
}

/// A box that has actually been measured: both sides positive.
pub(crate) fn measured(bounds: PaneBounds) -> Option<(f64, f64)> {
    (bounds.width > 0.0 && bounds.height > 0.0).then_some((bounds.width, bounds.height))
}

/// The CSS selector for one id. The reader's ids are ASCII words joined by
/// dashes (`page-list`, `sp-3`, `cont-12-pg`), which `#id` names directly;
/// anything else is quoted as an attribute match so an unusual id can never
/// turn into a malformed selector.
fn id_selector(id: &str) -> String {
    let plain = id
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain {
        format!("#{id}")
    } else {
        format!("[id=\"{}\"]", id.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_ids_select_by_hash_and_odd_ones_are_quoted() {
        assert_eq!(id_selector("page-list"), "#page-list");
        assert_eq!(id_selector("cont-12-pg"), "#cont-12-pg");
        assert_eq!(id_selector("3d"), "[id=\"3d\"]");
        assert_eq!(id_selector("a.b"), "[id=\"a.b\"]");
        assert_eq!(id_selector("q\"x"), "[id=\"q\\\"x\"]");
    }

    #[test]
    fn a_box_counts_as_measured_only_with_both_sides() {
        assert_eq!(measured(PaneBounds::filling(0.0, 600.0)), None);
        assert_eq!(measured(PaneBounds::filling(800.0, 0.0)), None);
        assert_eq!(
            measured(PaneBounds::filling(800.0, 600.0)),
            Some((800.0, 600.0))
        );
    }

    #[test]
    fn the_pane_hears_its_box_and_an_unmounted_root_answers_nothing() {
        let owner = Owner::new();
        owner.with(|| {
            let dom = PaneDom::default();
            assert_eq!(dom.measured_size(), None);
            dom.set_bounds(PaneBounds::filling(640.0, 480.0));
            assert_eq!(dom.measured_size(), Some((640.0, 480.0)));
            assert_eq!(dom.bounds(), PaneBounds::filling(640.0, 480.0));
        });
    }
}
