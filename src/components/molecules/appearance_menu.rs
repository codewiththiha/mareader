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

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::separator::Separator;
use crate::components::chrome::adaptive_group::{OverflowRow, ToolbarEntry};
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
pub fn AppearanceMenu(
    state: AppState,
    #[prop(optional)] open: Option<RwSignal<bool>>,
    #[prop(optional)] hide_trigger: Option<Signal<bool>>,
    #[prop(optional)] fallback_anchor: NodeRef<html::Div>,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let hide_trigger = hide_trigger.unwrap_or_else(|| Signal::derive(|| false));
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
                class=("hidden", move || hide_trigger.get())
            >
                <Icon name=IconName::Palette size=18 />
            </button>
            <Popover
                open=open
                anchor=root_ref
                fallback_anchor=fallback_anchor
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

pub fn appearance_entry(
    state: AppState,
    appearance_open: RwSignal<bool>,
    collapsed_ids: RwSignal<Vec<&'static str>>,
    overflow_ref: NodeRef<html::Div>,
) -> ToolbarEntry {
    ToolbarEntry {
        id: "appearance",
        priority: u32::MAX,
        keep_mounted: true,
        inline: Arc::new(move || {
            let hide = Signal::derive(move || {
                collapsed_ids.get().contains(&"appearance")
            });
            view! {
                <AppearanceMenu
                    state=state
                    open=appearance_open
                    hide_trigger=hide
                    fallback_anchor=overflow_ref
                />
            }
            .into_any()
        }),
        sizer: Arc::new(move || {
            view! {
                <button
                    type="button"
                    class="inline-flex items-center justify-center rounded-lg border h-9 px-2.5 text-sm font-medium border-line bg-surface text-ink"
                >
                    <Icon name=IconName::Palette size=18 />
                </button>
            }
            .into_any()
        }),
        collapsed: Arc::new(move |done| {
            view! {
                <OverflowRow icon=IconName::Palette label="Appearance…" done=done
                    on_click=move || {
                        request_animation_frame(move || appearance_open.set(true));
                    } />
            }
            .into_any()
        }),
    }
}
