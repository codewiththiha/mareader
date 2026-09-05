//! Waiting-state feedback: the centered loader and the shimmer that stands
//! in for content that has not arrived yet.


use leptos::prelude::*;

/// Default edge of the mark in CSS px. Big enough to read from across the
/// window when it sits alone in the middle of a surface.
const DEFAULT_SIZE: u32 = 72;

/// The three-dot loader. `size` is the square's edge in CSS px.
///
/// The dots are absolutely positioned inside the box the inline `width` gives
/// the parent, so `size` scales the whole mark — including the distance a dot
/// hops, which is a percentage of the dot itself.
#[component]
pub fn Loader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div
            class="loader"
            role="status"
            aria-label="Loading"
            style=format!("width:{size}px")
        >
            <span class="loader-dot loader-dot-a"></span>
            <span class="loader-dot loader-dot-b"></span>
            <span class="loader-dot loader-dot-c"></span>
        </div>
    }
}

/// The loader alone, centred in whatever box the caller gives it — the
/// shape every full-surface wait takes (opening a book, an empty panel
/// filling in). Fills its parent; the parent decides the extent.
#[component]
pub fn CenteredLoader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div class="flex h-full w-full items-center justify-center text-ink">
            <Loader size=size />
        </div>
    }
}

// ---------------------------------------------------------------------------
// Shimmer
// ---------------------------------------------------------------------------



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
