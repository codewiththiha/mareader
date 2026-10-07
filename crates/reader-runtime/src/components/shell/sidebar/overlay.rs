//! The floating rail's mount point: a fixed wrapper outside
//! `.reader-bg`, plus the edge-hover affordance.

use std::time::Duration;

use leptos::children::ChildrenFn;
use leptos::prelude::*;

use app_chrome::hooks::{HoverConfig, use_hover_reveal};
use app_ui::components::shell::controller::ShellController;

/// How long the pointer may be off the rail before it closes.
const HOVER_GRACE_MS: u64 = 250;

// `ChildrenFn`, not `Children`: `Show` re-runs its `Fn` closure.
#[component]
pub fn OverlayRail(shell: ShellController, children: ChildrenFn) -> impl IntoView {
    // Shown while the pointer is over the strip or the rail.
    let hover = use_hover_reveal(HoverConfig {
        delay: Duration::from_millis(HOVER_GRACE_MS),
        hold: Some(Signal::derive(move || !shell.is_overlay().get())),
        pin: None,
    });

    let visible = hover.visible;

    // Edge-triggered: only a `visible` flip acts, in overlay mode.
    let prev_vis = StoredValue::new_local(false);
    Effect::new(move |_| {
        let vis = visible.get();
        let was = prev_vis.get_value();
        prev_vis.set_value(vis);
        if !shell.is_overlay().get() {
            return;
        }
        if vis && !was && !shell.is_sidebar_open().get() {
            shell.open_last_panel();
        } else if !vis && was && shell.is_sidebar_open().get() {
            shell.close_sidebar();
        }
    });

    // The view bumps Copy counters; these effects relay them.
    let request_show = RwSignal::new(0u32);
    let request_hide = RwSignal::new(0u32);
    let (enter, leave) = hover.bind();
    Effect::new(move |_| {
        if request_show.get() > 0 {
            enter();
        }
    });
    Effect::new(move |_| {
        if request_hide.get() > 0 {
            leave();
        }
    });

    view! {
        // The edge strip: a hairline, only while the rail is fully closed.
        <Show when=move || shell.hover_strip_active().get()>
            <div
                class="fixed inset-y-0 left-0 z-[var(--z-bar)] w-1.5"
                on:mouseenter=move |_| request_show.update(|n| *n += 1)
            />
        </Show>
        <Show when=move || shell.is_overlay().get()>
            <div
                class=move || if shell.no_slide().get() { OVERLAY_STATIC } else { OVERLAY_FADES }
                class=("opacity-0", move || !shell.is_sidebar_open().get())
                class=("pointer-events-none", move || !shell.is_sidebar_open().get())
                on:mouseenter=move |_| request_show.update(|n| *n += 1)
                on:mouseleave=move |_| request_hide.update(|n| *n += 1)
            >
                {children()}
            </div>
        </Show>
    }
}

/// The floating rail's two class shapes, as literals.
const OVERLAY_STATIC: &str = "fixed inset-y-0 left-0 z-[var(--z-popover)] shadow-2xl";
const OVERLAY_FADES: &str = concat!(
    "fixed inset-y-0 left-0 z-[var(--z-popover)] shadow-2xl",
    " transition-opacity duration-200 ease-in-out"
);
