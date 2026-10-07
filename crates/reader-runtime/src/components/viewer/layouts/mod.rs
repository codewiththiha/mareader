//! The four view layouts, each rendering through the shared page hosts.

pub mod scroll_horizontal;
pub mod scroll_vertical;
pub mod single;
pub mod spread;

use leptos::prelude::*;

use crate::state::ReaderState;

/// Shared chrome the four layouts used to copy: inset, gap, progress strip.
pub struct LayoutChrome {
    pub inset: Signal<f64>,
    pub gap: Signal<f64>,
    pub progress_visible: Signal<bool>,
}

pub fn layout_chrome(state: ReaderState, progress_visible: Signal<bool>) -> LayoutChrome {
    LayoutChrome {
        inset: state.viewer.page_margin.read_only().into(),
        gap: state.viewer.page_gap.read_only().into(),
        progress_visible,
    }
}
