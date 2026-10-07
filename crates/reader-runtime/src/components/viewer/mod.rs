//! The viewer, organised by shape.

pub mod controls;
pub mod layouts;
pub mod page_host;
pub mod refresh;
pub mod shells;
pub mod texture_surface;

pub use page_host::{PageSlot, UniversalPageHost, UniversalStreamHost, UniversalStripHost};

use leptos::prelude::*;
use reader_core::view::ViewMode;
use virtual_list_leptos::Virtualizer;

use crate::components::viewer::layouts::scroll_horizontal::ScrollHorizontalLayout;
use crate::components::viewer::layouts::scroll_vertical::ScrollVerticalLayout;
use crate::components::viewer::layouts::single::SingleLayout;
use crate::components::viewer::layouts::spread::SpreadLayout;
use crate::state::ReaderState;

#[component]
pub fn Viewer(
    state: ReaderState,
    /// The continuous (vertical) reader's virtualizer, shared with navigation
    /// sync and the zoom coordinator.
    virtualizer: Virtualizer,
    /// The horizontal strip's virtualizer.
    h_virtualizer: Virtualizer,
    #[prop(into)] progress_visible: Signal<bool>,
) -> impl IntoView {
    // The virtualizers park in local storage for the dispatch closure.
    let virtualizer_view = StoredValue::new_local(virtualizer);
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer);
    let mode = state.viewer.mode;
    view! {
        {move || {
            match mode.get() {
                ViewMode::Single => view! {
                    <SingleLayout state=state progress_visible=progress_visible />
                }
                .into_any(),
                ViewMode::Spread => view! {
                    <SpreadLayout state=state progress_visible=progress_visible />
                }
                .into_any(),
                ViewMode::ScrollVertical => view! {
                    <ScrollVerticalLayout
                        state=state
                        virtualizer=virtualizer_view.get_value()
                        progress_visible=progress_visible
                    />
                }
                .into_any(),
                ViewMode::ScrollHorizontal => view! {
                    <ScrollHorizontalLayout
                        state=state
                        virtualizer=h_virtualizer_view.get_value()
                        progress_visible=progress_visible
                    />
                }
                .into_any(),
            }
        }}
    }
}
