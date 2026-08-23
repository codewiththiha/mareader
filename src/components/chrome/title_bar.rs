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

use std::rc::Rc;
use std::time::Duration;

use leptos::children::ViewFn;
use leptos::prelude::*;

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
    let hovered = RwSignal::new(false);
    let held_count = RwSignal::new(0usize);
    let is_held = Signal::derive(move || held_count.get() > 0);
    let visible = Signal::derive(move || pinned.get() || hovered.get());
    provide_context(TitleBarCtx { visible, held_count });

    let timer = StoredValue::new_local(None::<TimeoutHandle>);
    let show: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(h) = timer.get_value() {
            h.clear();
            timer.set_value(None);
        }
        hovered.set(true);
    });
    let hide_later = move || {
        // An open popover or the floating search pins the bar open.
        if is_held.get() || extra_hold.get() {
            return;
        }
        if let Some(h) = timer.get_value() {
            h.clear();
        }
        let h = set_timeout_with_handle(
            move || {
                if !is_held.get() && !extra_hold.get() {
                    hovered.set(false);
                }
            },
            Duration::from_millis(HIDE_DELAY_MS),
        )
        .ok();
        timer.set_value(h);
    };

    let show_band = show.clone();
    let show_bar = show;
    let sidebar_open = move || band_inset.get();

    view! {
        <>
            {children()}
            // Hover band = the whole titlebar area (grab zone), but NEVER over
            // the sidebar: `left-72` while the sidebar is open.
            <div
                class="absolute top-0 right-0 z-40 h-12"
                class=("left-72", sidebar_open)
                class=("left-0", move || !sidebar_open())
                data-tauri-drag-region="true"
                on:mouseenter=move |_| show_band()
                on:mouseleave=move |_| hide_later()
            >
                <div
                    // DocumentTitle measurement anchors MUST keep these ids.
                    id="toolbar-row"
                    data-tauri-drag-region="true"
                    prop:inert=move || !visible.get()
                    on:mouseenter=move |_| show_bar()
                    on:mouseleave=move |_| hide_later()
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
