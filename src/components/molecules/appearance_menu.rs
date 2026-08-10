//! Consolidated 🎨 Appearance menu (phase 3 / U6): theme + page texture + film
//! grain in ONE popover, replacing the three separate toolbar buttons. The three
//! repurposed section renderers (`ThemeMenu`, `TextureMenu`, `NoiseToggle`) are
//! stacked here under small section headers.
//!
//! Dismissal rules (the audit's key fix):
//! - Theme selection closes the popover (a `theme_id` watcher fires).
//! - Texture selection stays open (deliberate).
//! - Noise toggle/slider NEVER close it — the slider must stay usable mid-drag.
//! - Outside-click and Escape close it. This also gives menu-exclusivity:
//!   pointerdown on any other toolbar trigger lands outside this root, closing
//!   this popover first, then the click opens the other.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::html;
use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::separator::Separator;
use crate::core::state::AppState;

use super::noise_toggle::NoiseToggle;
use super::texture_menu::TextureMenu;
use super::theme_menu::ThemeMenu;

#[component]
pub fn AppearanceMenu(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    let trigger_class = move || {
        let base = "inline-flex items-center justify-center rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";
        if open.get() {
            format!("{base} border-accent text-accent")
        } else {
            base.to_string()
        }
    };

    // Close on theme selection only. Reading `theme_id` subscribes to every
    // settings write (texture + noise included), but we only fire when the theme
    // actually changes — the same prev-tracking pattern NoiseToggle used.
    let prev_theme: Rc<RefCell<Option<String>>> =
        Rc::new(RefCell::new(Some(state.settings.read_untracked().theme_id.clone())));
    Effect::new(move || {
        let theme_id = state.settings.get().theme_id;
        let mut prev = prev_theme.borrow_mut();
        if prev.as_ref() != Some(&theme_id) {
            *prev = Some(theme_id);
            open.set(false);
        }
    });

    // While open: outside-click and Escape close it. Re-registered per open via
    // an Effect (reads `open`); the previous run's cleanup removes the listeners.
    Effect::new(move |_| {
        if open.get() {
            let container = root_ref.get();
            let pointerdown = window_event_listener(
                leptos::ev::pointerdown,
                move |ev: leptos::ev::PointerEvent| {
                    let target: web_sys::Node = event_target(&ev);
                    let contains = container
                        .as_ref()
                        .map_or(false, |c| c.contains(Some(&target)));
                    if !contains {
                        open.set(false);
                    }
                },
            );
            let keydown = window_event_listener(
                leptos::ev::keydown,
                move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        open.set(false);
                    }
                },
            );
            on_cleanup(move || {
                pointerdown.remove();
                keydown.remove();
            });
        }
    });

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                title="Appearance"
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                <Icon name=IconName::Palette size=16 />
            </button>
            <Show when=move || open.get()>
                <div class="menu-popover absolute right-0 top-full z-50 mt-1 w-60 rounded-lg border border-line bg-surface p-3 shadow-lg">
                    <p class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">"Theme"</p>
                    <ThemeMenu state=state.clone() />
                    <div class="my-3"><Separator vertical=false /></div>
                    <p class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">"Page texture"</p>
                    <TextureMenu state=state.clone() />
                    <div class="my-3"><Separator vertical=false /></div>
                    <p class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">"Film grain"</p>
                    <NoiseToggle state=state.clone() />
                </div>
            </Show>
        </div>
    }
}
