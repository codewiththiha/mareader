//! Loading skeleton — generic UI feedback, not AI-specific. Born in the AI
//! word-info card; reusable for thumbnail loading, search loading, library
//! loading and preset loading.

use leptos::prelude::*;

/// Placeholder shimmer lines shown while content is loading.
#[component]
pub fn LoadingShimmer() -> impl IntoView {
    view! {
        <div class="flex flex-col gap-3 p-3" aria-label="Loading">
            <div class="ai-shimmer-line" style="width: 40%"></div>
            <div class="ai-shimmer-line" style="width: 90%"></div>
            <div class="ai-shimmer-line" style="width: 75%"></div>
            <div class="ai-shimmer-line" style="width: 60%"></div>
        </div>
    }
}
