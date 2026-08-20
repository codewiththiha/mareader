//! The 🎨 Appearance popover: presets, base mode + tint, texture, film grain.
//!
//! Dismissal rules (owned by the shared window-aware `Popover`):
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
//! The panel scrolls and is clamped/flipped by the Popover, so it can never
//! overflow off-screen.

use leptos::html;
use leptos::prelude::*;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::separator::Separator;
use crate::components::chrome::popover::Popover;
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

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                title="Appearance"
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                <Icon name=IconName::Palette size=18 />
            </button>
            <Popover
                open=open
                anchor=root_ref
                width=288
                class="max-h-[min(70vh,32rem)] overflow-y-auto p-3".to_string()
            >
                <SectionLabel text="Presets" />
                <PresetSection state=state />
                <div class="my-3"><Separator vertical=false /></div>
                <SectionLabel text="Mode & colour" />
                <BaseSection state=state />
                <div class="my-3"><Separator vertical=false /></div>
                <SectionLabel text="Page texture" />
                <TextureSection state=state />
                <div class="my-3"><Separator vertical=false /></div>
                <SectionLabel text="Film grain" />
                <NoiseSection state=state />
            </Popover>
        </div>
    }
}
