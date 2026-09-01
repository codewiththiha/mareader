//! The single dispatch point that picks a layout for the active `ViewMode`.
//! The layouts it renders are thin; the shells they use own the shared chrome.

use leptos::prelude::*;
use pdf_core::layout::ViewMode;
use virtual_list_leptos::Virtualizer;

use crate::components::document::layouts::scroll_horizontal::ScrollHorizontalLayout;
use crate::components::document::layouts::scroll_vertical::ScrollVerticalLayout;
use crate::components::document::layouts::single::SingleLayout;
use crate::components::document::layouts::spread::SpreadLayout;
use crate::state::ReaderState;

#[component]
pub fn Viewer(
    state: ReaderState,
    /// The continuous (vertical) reader's virtualizer, shared with navigation
    /// sync and the zoom coordinator.
    virtualizer: Virtualizer,
    /// The horizontal strip's virtualizer (same role when mode is scroll-horizontal).
    h_virtualizer: Virtualizer,
    #[prop(into)]
    progress_visible: Signal<bool>,
) -> impl IntoView {
    // The virtualizers are parked in local (non-thread-safe) storage so the
    // reactive dispatch closure only has to capture Send-friendly handles,
    // resolving them lazily for the branch actually shown.
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
