//! The word card's body: the AI answer sections.

use std::sync::Arc;

use ai_core::types::WordInfo;
use leptos::prelude::*;
use reader_core::settings::GlossDensity;

use app_ui::components::primitives::feedback::LoadingShimmer;

/// The body's density-dependent class sets.
fn section_classes(density: GlossDensity) -> (&'static str, &'static str, &'static str) {
    match density {
        GlossDensity::Compact => ("gap-2", "leading-normal", "gap-1.5"),
        GlossDensity::Comfortable => ("gap-3", "leading-relaxed", "gap-2"),
    }
}

/// Renders the AI result sections: Meaning, Synonyms, Usages.
#[component]
pub fn WordInfoSections(
    #[prop(into)] info: Signal<Option<Arc<WordInfo>>>,
    #[prop(into)] density: Signal<GlossDensity>,
) -> impl IntoView {
    let meaning = Memo::new(move |_| info.get().map(|i| i.meaning.clone()).unwrap_or_default());
    let synonyms = Memo::new(move |_| info.get().map(|i| i.synonyms.clone()).unwrap_or_default());
    let usages = Memo::new(move |_| info.get().map(|i| i.usages.clone()).unwrap_or_default());
    let has_synonyms = Signal::derive(move || !synonyms.get().is_empty());
    let has_usages = Signal::derive(move || !usages.get().is_empty());
    let classes = Signal::derive(move || section_classes(density.get()));

    view! {
        // The shimmer while the answer is still on its way.
        <Show when=move || info.get().is_none()>
            <LoadingShimmer />
        </Show>
        <Show when=move || info.get().is_some()>
            <div class=move || format!("flex flex-col {}", classes.get().0)>
                // ── Simplified Meaning ──────────────────────────────────
                <div class="ai-text-reveal ai-reveal-delay-1">
                    <div class="ai-section-label">"Meaning"</div>
                    <p class=move || format!("text-sm text-ink {}", classes.get().1)>
                        {move || meaning.get()}
                    </p>
                </div>

                // ── Synonyms ────────────────────────────────────────────
                <Show when=move || has_synonyms.get()>
                    <div class="ai-text-reveal ai-reveal-delay-2">
                        <div class="ai-section-label">"Synonyms"</div>
                        <div class="flex flex-wrap gap-1.5">
                            <For
                                each=move || synonyms.get()
                                key=|s: &String| s.clone()
                                children=move |synonym: String| {
                                    view! { <span class="ai-synonym-chip">{synonym}</span> }
                                }
                            />
                        </div>
                    </div>
                </Show>

                // ── Usage Examples ──────────────────────────────────────
                <Show when=move || has_usages.get()>
                    <div class="ai-text-reveal ai-reveal-delay-3">
                        <div class="ai-section-label">"Usage"</div>
                        <div class=move || format!("flex flex-col {}", classes.get().2)>
                            <For
                                each=move || usages.get()
                                key=|u: &String| u.clone()
                                children=move |usage: String| {
                                    view! { <p class="ai-usage-item">{usage}</p> }
                                }
                            />
                        </div>
                    </div>
                </Show>
            </div>
        </Show>
    }
}
