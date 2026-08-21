//! Reusable hover/grab titlebar with a pin. `left`/`right` are render-prop
//! slots so each page composes whatever controls it needs; the provider owns
//! the hover/pin state, the traffic lights, and the drag/hover band.
//!
//! It WRAPS its children so descendants (the floating doc title, the slot
//! menus' popovers) can read the shared [`TitleBarCtx`] — leptos context
//! flows down the reactive tree, so a sibling overlay would not see it.
//!
//! Sidebar-aware: the hover band starts at `left-72` while the sidebar is
//! open, so hovering the sidebar never reveals the reader bar; the native
//! traffic lights follow `visible || sidebar_open`.

use std::rc::Rc;
use std::time::Duration;

use leptos::children::ViewFn;
use leptos::prelude::*;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_viewer::state::SidebarMode;
use crate::core::state::AppState;
use crate::util::storage::save_settings;

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
pub fn TitleBarProvider(
    state: AppState,
    #[prop(into)] left: ViewFn,
    #[prop(into)] right: ViewFn,
    children: Children,
) -> impl IntoView {
    let pinned = RwSignal::new(state.settings.get_untracked().titlebar_pinned);
    let hovered = RwSignal::new(false);
    let held_count = RwSignal::new(0usize);
    let is_held = Signal::derive(move || held_count.get() > 0);
    let visible = Signal::derive(move || pinned.get() || hovered.get());
    provide_context(TitleBarCtx { visible, held_count });

    // Persist the pin.
    Effect::new(move |_| {
        let p = pinned.get();
        if state.settings.with(|s| s.titlebar_pinned) != p {
            state.settings.update(|s| s.titlebar_pinned = p);
            save_settings(&state.settings.get_untracked());
        }
    });

    // Traffic lights: on while pinned/hovered, or while an open sidebar owns
    // them (its chrome row is always visible).
    Effect::new(move |_| {
        let on = visible.get() || state.sidebar.get() != SidebarMode::None;
        wasm_bindgen_futures::spawn_local(async move {
            pdf_engine::api::set_traffic_lights(on).await;
        });
    });

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
        if is_held.get() || state.search.visible.get() {
            return;
        }
        if let Some(h) = timer.get_value() {
            h.clear();
        }
        let h = set_timeout_with_handle(
            move || {
                if !is_held.get() && !state.search.visible.get() {
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
    let sidebar_open = move || state.sidebar.get() != SidebarMode::None;

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
                    // DocTitle measurement anchors MUST keep these ids.
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
                        <PinButton pinned=pinned />
                    </div>
                </div>
            </div>
        </>
    }
}

/// Pin: while on, the bar never auto-hides. Persisted via Settings.
#[component]
fn PinButton(pinned: RwSignal<bool>) -> impl IntoView {
    view! {
        <Tooltip text="Pin titlebar open".to_string()>
            <button
                type="button"
                aria-pressed=move || pinned.get().to_string()
                on:click=move |_| pinned.set(!pinned.get())
                class="inline-flex h-9 w-9 items-center justify-center rounded-lg border border-transparent bg-transparent transition-colors hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                class=("text-accent", move || pinned.get())
                class=("text-ink", move || !pinned.get())
            >
                <Icon name=IconName::Pin size=18 />
            </button>
        </Tooltip>
    }
}
