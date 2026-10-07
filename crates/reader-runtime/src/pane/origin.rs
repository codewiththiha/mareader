//! Whose event this is: the one answer every pane-scoped listener
//! shares.

use crate::pane::dom::PaneDom;

/// The attribute every pane's content root carries (`closest`).
const PANE_ROOT_ATTR: &str = "data-pane-root";

/// Whose an event is, from one pane's point of view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Mine,
    Other,
}

/// `Some(true)` inside this pane, `Some(false)` another's, `None`
/// in no pane.
pub(crate) fn decide(in_pane: Option<bool>, active: bool) -> Origin {
    match in_pane {
        Some(true) => Origin::Mine,
        Some(false) => Origin::Other,
        None if active => Origin::Mine,
        None => Origin::Other,
    }
}

/// Classify an event dispatched on `element` for this pane.
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

/// For an event always raised inside a pane: is it this pane's?
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
