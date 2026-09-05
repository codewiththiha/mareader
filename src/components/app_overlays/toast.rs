//! App-global toast host. Renders the single current toast (if any) centered
//! near the top of the app and auto-dismisses it. The open-flow (toolbar)
//! emits error toasts here; the host is mounted at the app root (app.rs), so
//! its `position: fixed` is relative to the viewport rather than a
//! `backdrop-blur` ancestor's containing block.
//!
//! Everything except the state mapping is the `overlay` primitive: this file
//! maps `state.ui.toast` onto the slot host (`use_toast_slot` + the primitive
//! `ToastHost` view) — the toast id doubles as the equality guard, so a stale
//! timer can never wipe a newer toast.

use leptos::prelude::*;

use crate::components::primitives::overlay::toast::{ToastData, ToastTone};
use crate::components::primitives::overlay::toast_host::use_toast_slot;
use crate::components::primitives::overlay::toast_host::ToastHost as PrimitiveToastHost;
use crate::state::AppState;

#[component]
pub fn ToastHost(state: AppState) -> impl IntoView {
    // The slot source: one current toast, error tone.
    let source = Signal::derive(move || {
        state
            .ui
            .toast
            .get()
            .map(|t| ToastData::new(t.id, t.message, ToastTone::Error))
    });

    // Auto-dismiss, equality-guarded by id: whenever the toast changes, a
    // timer for THAT toast's duration is armed; at fire time it clears only
    // if the same toast is still current.
    use_toast_slot(
        source,
        move |id| state.ui.toast.with_untracked(|t| t.as_ref().is_some_and(|t| t.id == id)),
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
