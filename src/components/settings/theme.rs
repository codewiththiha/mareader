//! The Theme tab of the reader settings modal.
//!
//! The tab itself owns almost nothing: its two sections belong to the features
//! they configure, and this module is the composition — the AI's appearance
//! knobs ([`crate::components::ai::settings::AiAppearanceSection`]), the raster
//! paper and pipeline knobs ([`crate::components::settings::paper::PaperSection`],
//! which gate themselves on a PDF being open), and the pointer to the palette
//! menu for everything about the reader's own colour.

use leptos::prelude::*;

use crate::components::ai::settings::AiAppearanceSection;
use crate::components::primitives::menu::separator::Separator;
use crate::components::settings::paper::PaperSection;
use crate::state::AppState;

#[component]
pub(crate) fn ThemeTab(state: AppState) -> impl IntoView {
    view! {
        <AiAppearanceSection state=state />
        <PaperSection state=state />
        <div class="mt-5"><Separator vertical=false /></div>
        <p class="mt-2 text-xs text-muted">
            "Colour, tint, textures and presets live in the palette menu on the title bar."
        </p>
    }
}
