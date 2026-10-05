//! The four view layouts. Each is thin and shaped identically: it renders its
//! page arrangement inside the right shell, through the hosts that pick a format
//! ([`UniversalPageHost`](crate::components::viewer::page_host::UniversalPageHost)
//! and [`UniversalStripHost`](crate::components::viewer::page_host::UniversalStripHost)),
//! with zero scroll or zoom logic of its own.

pub mod scroll_horizontal;
pub mod scroll_vertical;
pub mod single;
pub mod spread;

use leptos::prelude::*;

use crate::state::ReaderState;

/// Shared chrome the four layouts used to copy: page inset, inter-page gap,
/// and whether the reading-progress strip is on. The shells consume this so
/// Single/Spread/Scroll* stay free of the same three `let`s.
#[derive(Clone, Copy)]
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
