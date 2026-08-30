//! Generic hover/grab titlebar shell. `left`/`right` are render-prop slots
//! so each page composes whatever controls it needs; the shell owns the
//! hover/pin state, the hide timers and the drag/hover band.
//!
//! It WRAPS its children so descendants (the floating doc title, the slot
//! menus' popovers) can read the shared [`TitleBarCtx`] — leptos context
//! flows down the reactive tree, so a sibling overlay would not see it.
//!
//! The shell knows nothing about the application: pin state, the native
//! traffic lights, sidebar insets and search holds arrive as props/signals
//! computed by `app_title_bar.rs` from the shell controller.

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
    /// True while something on the left (a DOCKED sidebar) owns the left
    /// inset, so the hover band starts at `left-72` (the rail's `w-72`). A
    /// rail that floats over the bar takes the corner from it instead, and
    /// the band keeps the full window width.
    band_inset: Signal<bool>,
    /// The row's left padding in px — the 88px traffic-light gutter while
    /// the bar owes the lights one, the resting padding (`pl-3` equivalent)
    /// once the corner belongs to something else. Computed by the shell
    /// controller's `titlebar_left_gutter`, which owns the rule.
    #[prop(into)] left_gutter: Signal<f64>,
    #[prop(into)] left: ViewFn,
    /// Centered overlay (e.g. the document title). Absolute-positioned over the
    /// row so left/right clusters keep their natural layout. Defaults to empty.
    #[prop(into, default = ViewFn::from(|| ()))]
    center: ViewFn,
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
    let bar_hovered = hover.visible;
    let visible = Signal::derive(move || pinned.get() || bar_hovered.get());
    provide_context(TitleBarCtx { visible, held_count });

    let hovered = StoredValue::new_local(false);
    let enter = {
        let show = hover.show.clone();
        move || {
            hovered.set_value(true);
            show();
        }
    };
    let leave = {
        let hide = hover.hide_later.clone();
        move || {
            hovered.set_value(false);
            hide();
        }
    };
    let recheck = hover.hide_later.clone();
    Effect::new(move |_| {
        let _ = is_held.get();
        let _ = extra_hold.get();
        if !is_held.get() && !extra_hold.get() && !hovered.get_value() {
            recheck(); // postpone is now false → schedules the hide
        }
    });

    let enter_band = enter.clone();
    let leave_band = leave.clone();
    let enter_bar = enter;
    let leave_bar = leave;
    let sidebar_open = move || band_inset.get();

    view! {
        <>
            {children()}
            // Hover band = the whole titlebar area (grab zone), but NEVER over
            // a DOCKED sidebar: `left-72` while it is open. A floating rail
            // paints above the band, so the band stays full width under it.
            <div
                class=format!("absolute top-0 right-0 {BAR} h-12")
                class=("left-72", sidebar_open)
                class=("left-0", move || !sidebar_open())
                data-tauri-drag-region="true"
                on:mouseenter=move |_| enter_band()
                on:mouseleave=move |_| leave_band()
            >
                <div
                    // DocumentTitle measurement anchors MUST keep these ids.
                    id=TOOLBAR_ROW_ID
                    data-tauri-drag-region="true"
                    prop:inert=move || !visible.get()
                    on:mouseenter=move |_| enter_bar()
                    on:mouseleave=move |_| leave_bar()
                    class="toolbar-glass relative flex h-full items-center gap-2 pr-2 transition-opacity duration-200"
                    // The px value is the controller's gutter rule; the
                    // trailing-comment contract it replaces lived on the
                    // old `pl-[88px]` / `pl-3` class toggles.
                    style:padding-left=move || format!("{}px", left_gutter.get())
                    class=("opacity-0", move || !visible.get())
                    class=("pointer-events-none", move || !visible.get())
                >
                    {left.run()}
                    <div
                        class="absolute inset-y-0 left-1/2 flex -translate-x-1/2 items-center"
                        data-tauri-drag-region="true"
                    >
                        {center.run()}
                    </div>
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
