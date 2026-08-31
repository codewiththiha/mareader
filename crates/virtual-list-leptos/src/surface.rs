//! Where scroll writes go. The core engine decides *what* to scroll to;
//! a [`ScrollSurface`] executes it. Splitting the two is what lets the
//! whole refresh engine run in plain host-side unit tests.
//!
//! Coordinates: surfaces receive **content coordinates** (`0` = top of the
//! first item). Negative values address the scrollable `padding_start` band.
//! Implementations translate to their own coordinate space (the DOM surface
//! adds `padding_start`).

use std::cell::RefCell;
use std::rc::Rc;

use crate::options::Axis;
use wasm_bindgen::JsCast;

/// Executes scroll commands issued by [`crate::engine::VirtualizerCore`].
pub trait ScrollSurface {
    /// Scroll so that content position `content_top` sits at the viewport's
    /// leading edge.
    fn set_scroll(&self, content_top: f64, smooth: bool);
}

/// The real surface: the bound scroll container element.
#[derive(Clone)]
pub struct DomSurface {
    slot: Rc<RefCell<Option<web_sys::Element>>>,
    axis: Axis,
    padding_start: f64,
}

impl DomSurface {
    /// A surface with no element attached yet.
    pub fn new(axis: Axis, padding_start: f64) -> Self {
        Self {
            slot: Rc::new(RefCell::new(None)),
            axis,
            padding_start,
        }
    }

    /// Attach (or replace) the scroll container.
    pub fn attach(&self, el: web_sys::Element) {
        *self.slot.borrow_mut() = Some(el);
    }

    /// Clear the currently attached element.
    pub fn detach(&self) {
        *self.slot.borrow_mut() = None;
    }

    /// The currently attached element, if any.
    pub fn element(&self) -> Option<web_sys::Element> {
        self.slot.borrow().clone()
    }
}

impl ScrollSurface for DomSurface {
    fn set_scroll(&self, content_top: f64, smooth: bool) {
        let Some(el) = self.element() else {
            return;
        };
        let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() else {
            return;
        };

        let opts = web_sys::ScrollToOptions::new();
        let offset = (content_top + self.padding_start).max(0.0);
        match self.axis {
            Axis::Vertical => opts.set_top(offset),
            Axis::Horizontal => opts.set_left(offset),
        }
        opts.set_behavior(if smooth {
            web_sys::ScrollBehavior::Smooth
        } else {
            web_sys::ScrollBehavior::Instant
        });
        html.scroll_to_with_scroll_to_options(&opts);
    }
}

/// Test double: records every write as `(content_top, smooth)`.
/// Host-test only — compiled just for the crate's unit tests.
#[derive(Default)]
#[cfg(test)]
pub(crate) struct TestSurface {
    writes: RefCell<Vec<(f64, bool)>>,
}

#[cfg(test)]
impl ScrollSurface for TestSurface {
    fn set_scroll(&self, content_top: f64, smooth: bool) {
        self.writes.borrow_mut().push((content_top, smooth));
    }
}

#[cfg(test)]
impl TestSurface {
    /// All writes so far.
    pub(crate) fn writes(&self) -> Vec<(f64, bool)> {
        self.writes.borrow().clone()
    }
}
