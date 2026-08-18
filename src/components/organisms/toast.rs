//! Toast notification host (U10). Renders the single current toast (if any)
//! centered near the top of the app and auto-dismisses it after ~3.5s. The
//! open-flow (toolbar) emits error toasts here; the host is mounted at the app
//! root (app.rs), so its `position: fixed` is relative to the viewport rather
//! than a `backdrop-blur` ancestor's containing block.

use std::time::Duration;

use leptos::prelude::*;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use crate::core::state::{AppState, ToastKind};

#[component]
pub fn ToastHost(state: AppState) -> impl IntoView {
    // Auto-dismiss: whenever the toast changes, arm a timer for the *current*
    // value. The callback clears the signal only if it still holds exactly
    // that toast (equality guard), so a stale timer never wipes a newer toast
    // that replaced the one it captured. The handle is cleared when the effect
    // re-runs or the component unmounts.
    Effect::new(move |_| {
        if let Some(t) = state.toast.get() {
            let captured = t.clone();
            let clear = state.toast;
            let handle = set_timeout_with_handle(
                move || {
                    if clear.get().as_ref() == Some(&captured) {
                        clear.set(None);
                    }
                },
                Duration::from_millis(3500),
            )
            .ok();
            on_cleanup(move || {
                if let Some(handle) = handle {
                    handle.clear();
                }
            });
        }
    });

    view! {
        // Centered host, click-through. `top-16` parks it just BELOW the 48px
        // toolbar header, so it never covers the toolbar's viewport-centered
        // PageNav (which also sits at 50% X). The wrapper is centered with
        // Tailwind's `-translate-x-1/2`; the toast inside is additionally
        // offset by `left-1/2` and re-centered by the keyframe's
        // `translate(-50%, ...)` (the keyframe keeps the -50% X so the centering
        // is maintained during the entrance slide, and `both` fill holds it once
        // the animation ends).
        <div class="pointer-events-none fixed left-1/2 top-16 z-[100] -translate-x-1/2">
            {move || {
                state.toast.get().map(|t| {
                    let (surface, icon) = match t.kind {
                        ToastKind::Info => {
                            ("border-line bg-surface/95 text-ink", IconName::Check)
                        }
                        ToastKind::Error => (
                            "border-red-400/50 bg-red-950/95 text-red-100",
                            IconName::Close,
                        ),
                    };
                    view! {
                        <div
                            class=format!(
                                "toast-enter relative left-1/2 flex max-w-[min(90vw,32rem)] items-center gap-2 rounded-xl border px-4 py-2.5 text-sm shadow-xl {surface}",
                            )
                        >
                            <Icon name=icon size=16 />
                            <span>{t.message.clone()}</span>
                        </div>
                    }
                })
            }}
        </div>
    }
}
