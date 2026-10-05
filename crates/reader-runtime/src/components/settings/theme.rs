//! The Theme tab of the reader settings modal.
//!
//! The tab itself owns almost nothing: its two sections belong to the features
//! they configure, and this module is the composition — the AI's appearance
//! knobs ([`crate::components::ai::settings::AiAppearanceSection`]), the raster
//! paper knobs ([`crate::components::settings::paper::PaperSection`], which
//! gate themselves on a PDF being open), and the pointer to the palette menu
//! for everything about the reader's own colour.

use leptos::prelude::*;

use crate::components::ai::settings::AiAppearanceSection;
use crate::components::settings::pane_appearance::PaneAppearanceSection;
use crate::components::settings::paper::PaperSection;
use app_ui::appearance::ThemeHandle;
use app_ui::components::primitives::menu::separator::Separator;

#[component]
pub(crate) fn ThemeTab(state: crate::context::ReaderContext) -> impl IntoView {
    let settings = state.settings;
    let theme = use_context::<ThemeHandle>().unwrap_or_else(|| ThemeHandle::for_settings(settings));
    let split = Signal::derive(move || theme.panes.get() >= 2);
    view! {
        <AiAppearanceSection state=state />
        <PaperSection state=state />
        <PaneAppearanceSection settings=settings visible=split />
        <Separator vertical=false spacing="mt-5" />
        <p class="mt-2 text-xs text-muted">
            "Reader theme colour, tint, textures and presets live in the palette menu on the title bar."
        </p>
    }
}
