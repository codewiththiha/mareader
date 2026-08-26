//! Small DOM helpers: animation-frame coalescing and viewport extraction.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;

use crate::options::Axis;
use virtual_list::Viewport;

/// Run `f` on the next animation frame. No-op off-wasm (host tests).
pub(crate) fn raf(f: impl FnOnce() + 'static) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let cb = Closure::once_into_js(f);
    let _ = win.request_animation_frame(cb.unchecked_ref());
}

/// The scrollport extents of an element as a [`Viewport`] for `axis`: `main`
/// is the scroll-axis extent, `cross` the extent across it.
pub(crate) fn viewport_of(el: &web_sys::Element, axis: Axis) -> Viewport {
    let Ok(html) = el.clone().dyn_into::<web_sys::HtmlElement>() else {
        return Viewport::main_only(0.0);
    };
    let (height, width) = (html.client_height() as f64, html.client_width() as f64);
    match axis {
        Axis::Vertical => Viewport::new(height, width),
        Axis::Horizontal => Viewport::new(width, height),
    }
}
