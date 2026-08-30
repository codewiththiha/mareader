//! The word card's BODY: the AI answer sections (meaning, synonyms, usages)
//! rendered at the chosen density. Sits inside `gloss/` because the gloss
//! surface is its only consumer — the measure twin renders it headless to
//! predict the card's height.

use leptos::prelude::*;
use pdf_core::settings::GlossDensity;

use crate::components::ai::types::WordInfo;
use crate::components::primitives::feedback::shimmer::LoadingShimmer;

/// The body's density-dependent class sets: the gap between sections, the
/// meaning's line height and the gap between usage examples. One tuple per
/// density so the surface and the measure twin can never disagree — the
/// twin's height is only correct if it renders EXACTLY what the card renders.
fn section_classes(density: GlossDensity) -> (&'static str, &'static str, &'static str) {
    match density {
        GlossDensity::Compact => ("gap-2", "leading-normal", "gap-1.5"),
        GlossDensity::Comfortable => ("gap-3", "leading-relaxed", "gap-2"),
    }
}

/// Renders the AI result sections: Meaning, Synonyms, Usages. The
/// part of speech is not one of them — it lives in the card's header, next to
/// the word it belongs to, the way a dictionary prints it.
///
/// Takes the answer as a SIGNAL, not a value: the backend streams partial
/// snapshots, and a value prop would rebuild this whole tree (and replay the
/// entrance animation) on every one of them — the flicker this component used
/// to have. Reading through derived signals means a snapshot PATCHES the text
/// in place; the reveal animation plays once, on mount.
#[component]
pub fn WordInfoSections(
    #[prop(into)] info: Signal<Option<WordInfo>>,
    #[prop(into)] density: Signal<GlossDensity>,
) -> impl IntoView {
    let meaning = Memo::new(move |_| info.get().map(|i| i.meaning).unwrap_or_default());
    let synonyms = Memo::new(move |_| info.get().map(|i| i.synonyms).unwrap_or_default());
    let usages = Memo::new(move |_| info.get().map(|i| i.usages).unwrap_or_default());
    let has_synonyms = Signal::derive(move || !synonyms.get().is_empty());
    let has_usages = Signal::derive(move || !usages.get().is_empty());
    let classes = Signal::derive(move || section_classes(density.get()));

    view! {
        // The shimmer while the answer is still on its way — mounted by the
        // same signal the sections patch through, so there is no phase read
        // here to re-run this view.
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
