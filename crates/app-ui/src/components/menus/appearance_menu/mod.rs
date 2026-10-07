//! The 🎨 Appearance popover: presets, base, tint, texture, grain.

use leptos::html;
use leptos::prelude::*;

use crate::appearance::{ThemeHandle, set_appearance_menu_open};
use crate::components::primitives::controls::button::{Button, ButtonVariant};
use crate::components::primitives::controls::switch::Switch;
use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::shell::controller::ChromeSurface;
use app_chrome::icon::{Icon, IconName};
use app_state::ChromeState;

// Structural changes go through the handle's `commit`, which flushes the
// pending scrub first.

mod hue_picker;
mod mode_section;
mod noise_section;
mod presets;
mod texture_section;

use mode_section::BaseSection;
use noise_section::NoiseSection;
use presets::PresetSection;
use texture_section::TextureSection;

#[component]
pub fn AppearanceMenu(
    state: ChromeState,
    #[prop(optional)] open: Option<RwSignal<bool>>,
    /// Which route's bar mounted the menu; the texture section asks it.
    #[prop(optional)]
    surface: ChromeSurface,
    /// The theme this menu edits.
    #[prop(optional)]
    theme: Option<ThemeHandle>,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let theme = theme.unwrap_or_else(|| ThemeHandle::for_settings(state.settings));
    // The independent-theme toggle appears with the split it serves.
    let split = Signal::derive(move || theme.panes.get() >= 2);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    // The engine's raw-retention gate, open with the popover.
    Effect::new(move || {
        set_appearance_menu_open(open.get());
    });
    on_cleanup(|| set_appearance_menu_open(false));

    // The texture section's one fact: this surface has a document.
    let texture_applies = Signal::derive(move || surface == ChromeSurface::Reader);

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            // The toolbar-Button variant owns the trigger look.
            <div>
                <Button
                    on_click=move |_| open.set(!open.get())
                    variant=ButtonVariant::Toolbar
                    active=Signal::derive(move || open.get())
                    title="Appearance"
                >
                    <Icon name=IconName::Palette size=18 />
                </Button>
            </div>
            <MenuPopover
                open=open
                anchor=root_ref
                width=288u32
                coordinate_space="toolbar-row"
                class="max-h-[min(70vh,32rem)] overflow-y-auto p-3".to_string()
            >
                // The dials edit the selected mode's look; grain is global.
                <SectionLabel text="Presets" />
                <PresetSection state=state theme=theme />
                <Separator vertical=false spacing="my-3" />
                <SectionLabel text="Mode & colour" />
                <BaseSection state=state theme=theme />
                <Show when=move || texture_applies.get()>
                    <Separator vertical=false spacing="my-3" />
                    <div data-appearance-section="page-texture">
                        <SectionLabel text="Page texture" />
                        <TextureSection theme=theme />
                        <Show when=move || split.get()>
                            <div
                                class="mt-3 flex items-center justify-between gap-3"
                                data-setting="independent-textures"
                            >
                                <span class="min-w-0">
                                    <span class="block text-sm text-ink">"Texture for each"</span>
                                    <span class="block text-xs text-muted">
                                        "Each pane keeps its own texture and dials."
                                    </span>
                                </span>
                                <Switch
                                    checked=theme.independent_texture
                                    on_change=theme.set_independent_texture
                                    title="Independent page texture for each pane"
                                />
                            </div>
                        </Show>
                    </div>
                </Show>
                <Separator vertical=false spacing="my-3" />
                <div data-appearance-section="film-grain">
                    <SectionLabel text="Film grain" />
                    <NoiseSection state=state theme=theme />
                </div>
                <Show when=move || split.get()>
                    <Separator vertical=false spacing="my-3" />
                    <div
                        class="flex items-center justify-between gap-3"
                        data-setting="independent-themes"
                    >
                        <span class="min-w-0">
                            <span class="block text-sm text-ink">"Independent theme for each"</span>
                            <span class="block text-xs text-muted">
                                "Each pane keeps its own colour; grain stays shared."
                            </span>
                        </span>
                        <Switch
                            checked=theme.independent
                            on_change=theme.set_independent
                            title="Independent theme for each pane"
                        />
                    </div>
                </Show>
            </MenuPopover>
        </div>
    }
}
