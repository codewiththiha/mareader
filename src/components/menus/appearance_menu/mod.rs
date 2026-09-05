//! The 🎨 Appearance popover: presets, base mode + tint, texture, film grain.
//!
//! Dismissal rules (owned by the shared window-aware `Popover`):
//! - Outside-click and Escape close it.
//! - Exclusivity with every other floating surface is NOT a side effect of that
//!   outside press. It used to be — a press on another toolbar trigger landed
//!   outside this root, so this popover closed and then the other opened — and
//!   that story only ever held menu-to-menu: a modal is not a press target, and
//!   a trigger under a modal's backdrop is still clickable, so this menu and the
//!   settings modal could end up open at once. `MenuPopover` now registers this
//!   popover's open signal with the overlay board
//!   ([`crate::components::primitives::overlay::lanes`]) as
//!   [`OverlayPolicy::MENU`][crate::components::primitives::overlay::lanes::OverlayPolicy],
//!   and the board evicts whichever surface loses. Nothing here does that work.
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

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use app_chrome::icon::{Icon, IconName};
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::shell::titlebar::toolbar_popover::MenuPopover;
use crate::effects::appearance::flush_appearance_commit;
use crate::state::AppState;
use reader_core::settings::Settings;

/// A structural appearance change (base mode, texture mode, grain mode):
/// flush any slider scrub still pending so the values the reader was just
/// dialling land FIRST, then apply the change and mark the appearance dirty
/// for rebake/persist. Every section's option buttons go through here —
/// the flush preamble must not be re-typed per call site, or one forgotten
/// copy silently drops the reader's in-flight dial.
pub(crate) fn update_appearance(state: AppState, change: impl FnOnce(&mut Settings)) {
    flush_appearance_commit();
    state.settings.update(|s| {
        change(s);
        s.touch_appearance();
    });
}

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
    state: AppState,
    #[prop(optional)] open: Option<RwSignal<bool>>,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            // The toolbar-Button variant owns the trigger look (incl. the open
            // accent state). The wrapper div is the MenuPopover's anchor.
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
                width=288
                coordinate_space="toolbar-row"
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
            </MenuPopover>
        </div>
    }
}

