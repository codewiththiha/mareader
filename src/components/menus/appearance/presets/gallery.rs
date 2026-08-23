//! The grouped rows of preset thumbnails.

use leptos::prelude::*;

use crate::state::AppState;
use pdf_core::presets::PresetGroup;

use super::swatch::PresetSwatch;

#[component]
pub(super) fn PresetGallery(
    state: AppState,
    groups: impl Fn() -> Vec<PresetGroup> + Clone + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <For
            each=groups
            key=|g| format!("{}-{}", g.name, g.presets.len())
            children=move |g| {
                view! {
                    <div class="mb-2">
                        <div class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">
                            {g.name.clone()}
                        </div>
                        <div class="grid grid-cols-4 gap-1">
                            {g
                                .presets
                                .iter()
                                .map(|p| view! { <PresetSwatch preset=p.clone() state=state /> })
                                .collect_view()}
                        </div>
                    </div>
                }
            }
        />
    }
}
