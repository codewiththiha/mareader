//! Toast hosts: the auto-dismiss wiring around [`ToastData`](super::toast::ToastData)
//! plus the host view.
//!
//! The app is deliberately **single-slot**: one current toast, replace-on-push,
//! equality-guarded expiry so a stale timer never wipes a newer toast
//! ([`use_toast_slot`] with [`ToastHost`]). A concurrent stack (bulk
//! operations that push several toasts at once) can be layered on the same
//! `ToastData` plus id-guarding rules when a real consumer appears; the
//! gloss undo deliberately keeps its own *generation-guarded* batch
//! semantics (undo cannot be a bare auto-dismiss) but renders through
//! [`ToastPanel`](super::toast::ToastPanel).

use leptos::prelude::*;

use super::toast::ToastData;
use crate::components::primitives::floating::types::z::TOAST;

/// Arm an auto-dismiss for the *current* slot toast.
///
/// Whenever the slot value changes, a timer for that toast's duration is
/// armed (previous one cleared). At fire time, `still_current` decides
/// whether the toast it was armed for is still the one on screen — the
/// equality guard that keeps a stale timer from wiping a newer toast.
/// `on_expire` performs the actual clear.
pub fn use_toast_slot(
    source: Signal<Option<ToastData>>,
    still_current: impl Fn(u64) -> bool + 'static,
    on_expire: impl Fn(u64) + 'static,
) {
    let still_current = std::rc::Rc::new(still_current);
    let on_expire = std::rc::Rc::new(on_expire);
    Effect::new(move |_| {
        let Some(t) = source.get() else {
            return;
        };
        let Some(duration) = t.duration else {
            return;
        };
        let id = t.id;
        let still_current = std::rc::Rc::clone(&still_current);
        let on_expire = std::rc::Rc::clone(&on_expire);
        let handle = set_timeout_with_handle(
            move || {
                if still_current(id) {
                    on_expire(id);
                }
            },
            duration,
        )
        .ok();
        on_cleanup(move || {
            if let Some(handle) = handle {
                handle.clear();
            }
        });
    });
}

/// Single-slot toast host, centered near the top of the viewport
/// (click-through wrapper; each toast is interactive).
#[component]
pub fn ToastHost(toasts: Signal<Option<ToastData>>) -> impl IntoView {
    view! {
        <div class=format!("pointer-events-none fixed inset-x-0 top-14 {TOAST} flex flex-col items-center gap-2 px-4")>
            {move || {
                toasts.get().map(|t| view! { <super::toast::ToastPanel toast=t /> })
            }}
        </div>
    }
}
