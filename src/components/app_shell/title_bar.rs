//! Generic hover/grab titlebar shell. `left`/`right` are render-prop slots
//! so each page composes whatever controls it needs; the shell owns the
//! hover/pin state, the hide timers and the drag/hover band.
//!
//! It WRAPS its children so descendants (the floating doc title, the slot
//! menus' popovers) can read the shared [`TitleBarCtx`] — leptos context
//! flows down the reactive tree, so a sibling overlay would not see it.
//!
//! The shell knows nothing about the application: pin persistence, the
//! native traffic lights, sidebar insets and search holds are injected
//! through props/callbacks (see `app_title_bar.rs`).

use std::time::Duration;

use leptos::children::ViewFn;
use leptos::prelude::*;

use crate::components::primitives::floating::types::z::BAR;
use crate::components::primitives::hooks::dom::TOOLBAR_ROW_ID;
use crate::components::primitives::hooks::use_timeout::use_hover_visibility;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::tooltip::Tooltip;

/// Pointer must be off the bar this long before it hides.
const HIDE_DELAY_MS: u64 = 400;

/// Shared chrome state, provided to descendants (the floating doc title and
/// the slot menus' popovers).
#[derive(Clone, Copy)]
pub struct TitleBarCtx {
    /// Effective bar visibility = pinned OR hovered.
    pub visible: Signal<bool>,
    /// Active holds count from open popovers in the titlebar.
    pub held_count: RwSignal<usize>,
}

#[component]
pub fn TitleBar(
    /// Pinned state: while on, the bar never auto-hides.
    pinned: RwSignal<bool>,
    /// Called with the new value whenever the pin toggles (persistence).
    on_pin_change: Callback<bool>,
    /// Extra hold from outside the bar (e.g. the open floating search).
    extra_hold: Signal<bool>,
    /// True while something on the left (the sidebar) owns the left inset:
    /// the hover band starts at `left-72` and the row drops its traffic-light
    /// padding.
    band_inset: Signal<bool>,
    #[prop(into)] left: ViewFn,
    #[prop(into)] right: ViewFn,
    children: Children,
) -> impl IntoView {
    let held_count = RwSignal::new(0usize);
    let is_held = Signal::derive(move || held_count.get() > 0);
    // Show on enter, hide after a grace period unless something holds the bar
    // open (an open popover, the floating search). The shared primitive owns
    // the timer + re-check-both-ends semantics; the shell owns the hold
    // definition.
    let hover = use_hover_visibility(
        Duration::from_millis(HIDE_DELAY_MS),
        move || is_held.get() || extra_hold.get(),
    );
    let hovered = hover.visible;
    let visible = Signal::derive(move || pinned.get() || hovered.get());
    provide_context(TitleBarCtx { visible, held_count });

    let hide_later_band = hover.hide_later.clone();
    let hide_later_bar = hover.hide_later;
    let show_band = hover.show.clone();
    let show_bar = hover.show;
    let sidebar_open = move || band_inset.get();

    view! {
        <>
            {children()}
            // Hover band = the whole titlebar area (grab zone), but NEVER over
            // the sidebar: `left-72` while the sidebar is open.
            <div
                class=format!("absolute top-0 right-0 {BAR} h-12")
                class=("left-72", sidebar_open)
                class=("left-0", move || !sidebar_open())
                data-tauri-drag-region="true"
                on:mouseenter=move |_| show_band()
                on:mouseleave=move |_| hide_later_band()
            >
                <div
                    // DocumentTitle measurement anchors MUST keep these ids.
                    id=TOOLBAR_ROW_ID
                    data-tauri-drag-region="true"
                    prop:inert=move || !visible.get()
                    on:mouseenter=move |_| show_bar()
                    on:mouseleave=move |_| hide_later_bar()
                    class="toolbar-glass flex h-full items-center gap-2 pr-2 transition-opacity duration-200"
                    // 88px clears the lights (x:20 + ~54px) + a real gap.
                    class=("pl-[88px]", move || !sidebar_open())
                    class=("pl-3", sidebar_open)
                    class=("opacity-0", move || !visible.get())
                    class=("pointer-events-none", move || !visible.get())
                >
                    {left.run()}
                    <div class="ml-auto flex shrink-0 items-center gap-1">
                        {right.run()}
                        <PinButton pinned=pinned on_pin_change=on_pin_change />
                    </div>
                </div>
            </div>
        </>
    }
}

/// Pin: while on, the bar never auto-hides. The new value is reported to the
/// caller, which owns persistence.
#[component]
fn PinButton(
    pinned: RwSignal<bool>,
    on_pin_change: Callback<bool>,
) -> impl IntoView {
    view! {
        <Tooltip text="Pin titlebar open">
            <IconButton
                icon=IconName::Pin
                pressed=pinned.into()
                on_click=move || {
                    let next = !pinned.get();
                    pinned.set(next);
                    on_pin_change.run(next);
                }
            />
        </Tooltip>
    }
}
