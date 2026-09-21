//! The 🎨 Appearance popover: presets, base mode + tint, texture, film grain.
//!
//! Dismissal rules (owned by the shared window-aware `Popover`):
//! - Outside-click and Escape close it.
//! - Exclusivity with every other floating surface is NOT a side effect of that
//!   outside press: a modal is not a press target, and a trigger under a modal's
//!   backdrop is still clickable, so press-driven closing once left this menu and
//!   the settings modal open at once. `MenuPopover` registers this popover's open
//!   signal with the overlay board
//!   ([`crate::components::primitives::overlay::lanes`]) as
//!   [`OverlayPolicy::MENU`][crate::components::primitives::overlay::lanes::OverlayPolicy],
//!   and the board evicts whichever surface loses. Nothing here does that work.
//! - NOTHING inside closes it. Choosing a preset and then nudging its tint is the
//!   normal workflow, and a popover that vanished on the first click would make
//!   that impossible. Every control here is live-preview, so staying open IS the
//!   feedback loop.
//!
//! The panel scrolls and is clamped/flipped by the Popover, so it can never
//! overflow off-screen.
//!
//! ## Which sections show is a question about the surface
//!
//! Not every knob paints anything on every route, and a section that changes
//! something no pixel on screen reads is a section the reader has to read and
//! then ignore. So the menu is told WHICH surface mounted it
//! ([`ChromeSurface`], the shell controller's name for the route) and gates
//! the one section that is a document's business — page texture paints the
//! PDF's paper bitmaps, so it shows on the reader surface and only while a
//! raster document is the one open (the same two facts the settings modal's
//! Paper section gates itself on). The shelf has no page to texture; a
//! reflowable document paints its paper from the theme tokens. Mode, tint,
//! presets and grain are the window's own and show everywhere.

use leptos::html;
use leptos::prelude::*;

use crate::components::primitives::controls::button::{Button, ButtonVariant};
use app_chrome::controller::ChromeSurface;
use app_chrome::icon::{Icon, IconName};
use crate::components::primitives::menu::section_label::SectionLabel;
use crate::components::primitives::menu::separator::Separator;
use crate::components::primitives::floating::menu_popover::MenuPopover;
use crate::effects::appearance::{flush_appearance_commit, set_appearance_menu_open};
use crate::state::AppState;
use reader_core::settings::Settings;

/// A structural appearance change (base mode, texture mode, grain mode):
/// flush any slider scrub still pending so the values the reader was just
/// dialling land FIRST, then apply the change and mark the appearance dirty
/// for rebake/persist. Every section's option buttons go through here — the
/// flush preamble must not be re-typed per call site, or one forgotten copy
/// silently drops the reader's in-flight dial.
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
    /// Which route's bar mounted the menu. The reader's is the default; the
    /// shelf names itself, and the texture section stands down there — and on
    /// the reader too, while a reflowable document is the one open.
    #[prop(optional)] surface: ChromeSurface,
) -> impl IntoView {
    let open = open.unwrap_or_else(|| RwSignal::new(false));
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    // The engine's raw-retention gate: while this popover is open, pages
    // that finish rendering keep their unbaked rasters, so the first tint
    // drag of a session blits them under the live CSS instead of
    // re-rendering every page (src/effects/appearance/mod.rs). Unmount
    // resets the flag — the engine must not outlive the menu's claim on it,
    // and a route swap remounts whichever bar carries the menu next.
    Effect::new(move || {
        set_appearance_menu_open(open.get());
    });
    on_cleanup(|| set_appearance_menu_open(false));

    // The texture section's two facts, in one derive: this surface has pages
    // to texture, and the document open on it is a raster one. Tracked, so a
    // text document swapping in takes the section out (and a PDF swaps it
    // back) without the menu being remounted.
    let texture_applies = Signal::derive(move || {
        surface == ChromeSurface::Reader && !state.reader.reflowable()
    });

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
                width=288u32
                coordinate_space="toolbar-row"
                class="max-h-[min(70vh,32rem)] overflow-y-auto p-3".to_string()
            >
                <SectionLabel text="Presets" />
                <PresetSection state=state />
                <Separator vertical=false spacing="my-3" />
                <SectionLabel text="Mode & colour" />
                <BaseSection state=state />
                <Show when=move || texture_applies.get()>
                    <Separator vertical=false spacing="my-3" />
                    <SectionLabel text="Page texture" />
                    <TextureSection state=state />
                </Show>
                <Separator vertical=false spacing="my-3" />
                <SectionLabel text="Film grain" />
                <NoiseSection state=state />
            </MenuPopover>
        </div>
    }
}

