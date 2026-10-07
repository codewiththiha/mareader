//! App-global toast host: the slot host with the app's state mapping.

use leptos::prelude::*;

use crate::components::primitives::overlay::toast::{ToastData, ToastTone};
use crate::components::primitives::overlay::toast_host::ToastHost as PrimitiveToastHost;
use crate::components::primitives::overlay::toast_host::use_toast_slot;
use app_state::ChromeState;

#[component]
pub fn ToastHost(state: ChromeState) -> impl IntoView {
    // The slot source: one current toast, error tone.
    let source = Signal::derive(move || {
        state
            .ui
            .toast
            .get()
            .map(|t| ToastData::new(t.id, t.message, ToastTone::Error))
    });

    // Auto-dismiss, equality-guarded by id.
    use_toast_slot(
        source,
        move |id| {
            state
                .ui
                .toast
                .with_untracked(|t| t.as_ref().is_some_and(|t| t.id == id))
        },
        move |id| {
            state.ui.toast.update(|t| {
                if t.as_ref().is_some_and(|t| t.id == id) {
                    *t = None;
                }
            });
        },
    );

    // The primitive host owns the centering wrapper + toast panel.
    view! { <PrimitiveToastHost toasts=source /> }
}
