//! Whose event is this? The one answer every pane-scoped listener shares.
//!
//! Some events still arrive on the window — the engine's link jump and
//! selection events, the gloss requests — because the thing that raises them
//! (a bundled IIFE, a component deep in the tree) holds no pane handle. With
//! two panes live, each installs its own listener, and every listener hears
//! every event. The raiser therefore dispatches ON the element the event
//! came from and lets it bubble, and each listener asks here whether that
//! element is inside its own pane's root:
//!
//! * inside MY root → [`Origin::Mine`];
//! * inside ANOTHER pane's root → [`Origin::Other`];
//! * inside no pane at all (chrome, the window itself) → mine only if I am
//!   the active pane, so exactly one pane answers.
//!
//! What a listener does with `Other` is its own policy: a link jump or a
//! gloss request ignores it; a selection in another pane means there is no
//! selection in this one (the document has one selection, realm-wide).

use crate::pane::dom::PaneDom;

/// The attribute every pane's content root carries, so an event's element
/// can find the pane it is in (`closest`). Written as a literal by the
/// pane's own view; only the selector here names it.
const PANE_ROOT_ATTR: &str = "data-pane-root";

/// Whose an event is, from one pane's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Mine,
    Other,
}

/// The rule, over what the DOM said: `in_pane` is `Some(true)` when the
/// element is inside this pane's root, `Some(false)` when it is inside
/// another pane's, `None` when it is inside no pane.
pub(crate) fn decide(in_pane: Option<bool>, active: bool) -> Origin {
    match in_pane {
        Some(true) => Origin::Mine,
        Some(false) => Origin::Other,
        None if active => Origin::Mine,
        None => Origin::Other,
    }
}

/// Classify an event dispatched on `element` (`None`: on the window) for
/// the pane that owns `dom`, which is the host's active pane when `active`.
pub fn origin_of(dom: &PaneDom, active: bool, element: Option<&web_sys::Element>) -> Origin {
    let selector = format!("[{PANE_ROOT_ATTR}]");
    let in_pane = element
        .and_then(|el| el.closest(&selector).ok().flatten())
        .map(|root| {
            dom.root()
                .is_some_and(|mine| mine.is_same_node(Some(root.as_ref())))
        });
    decide(in_pane, active)
}

/// For an event that is always raised INSIDE some pane (a gloss request,
/// raised on the stroke or the pill that asked): is it this pane's? No
/// active-pane fallback — an event raised on nothing in particular is
/// nobody's.
pub fn raised_in(dom: &PaneDom, element: Option<&web_sys::Element>) -> bool {
    origin_of(dom, false, element) == Origin::Mine
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_element_in_a_pane_belongs_to_that_pane_whatever_is_active() {
        assert_eq!(decide(Some(true), false), Origin::Mine);
        assert_eq!(decide(Some(true), true), Origin::Mine);
        assert_eq!(decide(Some(false), true), Origin::Other);
        assert_eq!(decide(Some(false), false), Origin::Other);
    }

    #[test]
    fn an_element_in_no_pane_belongs_to_the_active_pane_only() {
        assert_eq!(decide(None, true), Origin::Mine);
        assert_eq!(decide(None, false), Origin::Other);
    }
}
