//! Toast hosts: the auto-dismiss wiring around [`ToastData`].

use leptos::prelude::*;

use super::toast::ToastData;
use app_chrome::layers::TOAST;

/// Arm an auto-dismiss for the *current* slot toast; `still_current`
/// guards the fire.
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

/// Single-slot toast host, bottom-center of the viewport.
#[component]
pub fn ToastHost(toasts: Signal<Option<ToastData>>) -> impl IntoView {
    view! {
        <div class=format!("toast-anchor pointer-events-none {TOAST}")>
            {move || {
                toasts.get().map(|t| view! { <super::toast::ToastPanel toast=t /> })
            }}
        </div>
    }
}
