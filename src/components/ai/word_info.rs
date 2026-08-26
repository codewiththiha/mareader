use leptos::prelude::*;

use super::types::WordInfo;

/// Renders the four AI result sections: POS, Meaning, Synonyms, Usages.
/// Each section gets the `.ai-text-reveal` class and a staggered delay
/// so they cascade in rather than appearing all at once.
#[component]
pub fn WordInfoSections(info: WordInfo) -> impl IntoView {
    // The view closures below each need their own copy, so split the struct
    // up front (the emptiness flags stay `Copy` for the `Show` conditions).
    // The vecs go into `StoredValue` (`Copy`): `Show` children must be `Fn`,
    // and a `move` closure capturing the vec directly would be `FnOnce`.
    let has_synonyms = !info.synonyms.is_empty();
    let has_usages = !info.usages.is_empty();
    let WordInfo {
        pos,
        meaning,
        synonyms,
        usages,
    } = info;
    let synonyms = StoredValue::new(synonyms);
    let usages = StoredValue::new(usages);

    view! {
        <div class="flex flex-col gap-3 p-3">

            // ── Part of Speech ──────────────────────────────────────
            <div class="ai-text-reveal ai-reveal-delay-1">
                <div class="ai-section-label">"Part of Speech"</div>
                <span class="ai-pos-badge">{pos}</span>
            </div>

            // ── Simplified Meaning ──────────────────────────────────
            <div class="ai-text-reveal ai-reveal-delay-2">
                <div class="ai-section-label">"Meaning"</div>
                <p class="text-sm leading-relaxed text-ink">{meaning}</p>
            </div>

            // ── Synonyms ────────────────────────────────────────────
            <Show when=move || has_synonyms>
                <div class="ai-text-reveal ai-reveal-delay-3">
                    <div class="ai-section-label">"Synonyms"</div>
                    <div class="flex flex-wrap gap-1.5">
                        <For
                            each=move || synonyms.get_value()
                            key=|s: &String| s.clone()
                            children=move |synonym: String| {
                                view! { <span class="ai-synonym-chip">{synonym}</span> }
                            }
                        />
                    </div>
                </div>
            </Show>

            // ── Usage Examples ──────────────────────────────────────
            <Show when=move || has_usages>
                <div class="ai-text-reveal ai-reveal-delay-4">
                    <div class="ai-section-label">"Usage"</div>
                    <div class="flex flex-col gap-2">
                        <For
                            each=move || usages.get_value()
                            key=|u: &String| u.clone()
                            children=move |usage: String| {
                                view! { <p class="ai-usage-item">{usage}</p> }
                            }
                        />
                    </div>
                </div>
            </Show>
        </div>
    }
}

