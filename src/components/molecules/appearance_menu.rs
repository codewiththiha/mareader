//! The 🎨 Appearance popover: presets, base mode + tint, texture, film grain.
//!
//! Dismissal rules:
//! - Outside-click and Escape close it. This also gives menu-exclusivity:
//!   pointerdown on any other toolbar trigger lands outside this root, closing
//!   this popover first, then the click opens the other.
//! - NOTHING inside closes it. The old menu closed on theme selection, which
//!   made sense when a theme was one click and you were done. It is actively
//!   wrong now: choosing a preset and then nudging its tint is the normal
//!   workflow, and a popover that vanished on the first click would make that
//!   impossible. Every control here is live-preview, so staying open IS the
//!   feedback loop.
//!
//! The popover scrolls: with five sections it can exceed the viewport on a
//! short window, and a menu that overflows off-screen loses its bottom
//! controls entirely.

use leptos::html;
use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::separator::Separator;
use crate::core::state::AppState;

use super::appearance::base_section::BaseSection;
use super::appearance::noise_section::NoiseSection;
use super::appearance::preset_section::PresetSection;
use super::appearance::texture_section::TextureSection;

#[component]
fn SectionLabel(#[prop(into)] text: String) -> impl IntoView {
    view! {
        <p class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">{text}</p>
    }
}

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
                <div class="menu-popover absolute right-0 top-full z-50 mt-1 max-h-[min(70vh,32rem)] w-72 overflow-y-auto rounded-lg border border-line bg-surface p-3 shadow-lg">
                    <SectionLabel text="Presets" />
                    <PresetSection state=state.clone() />
                    <div class="my-3"><Separator vertical=false /></div>
                    <SectionLabel text="Mode & colour" />
                    <BaseSection state=state.clone() />
                    <div class="my-3"><Separator vertical=false /></div>
                    <SectionLabel text="Page texture" />
                    <TextureSection state=state.clone() />
                    <div class="my-3"><Separator vertical=false /></div>
                    <SectionLabel text="Film grain" />
                    <NoiseSection state=state.clone() />
                </div>
            </Show>
        </div>
    }
}
