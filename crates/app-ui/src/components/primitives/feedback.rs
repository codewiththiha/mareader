//! Waiting-state feedback: the centered loader and the shimmer.

use leptos::prelude::*;

/// Default edge of the mark in CSS px.
const DEFAULT_SIZE: u32 = 72;

/// The three-dot loader; `size` is the square's edge in CSS px.
#[component]
fn Loader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
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

/// The loader alone, centred in whatever box the caller gives it.
#[component]
pub fn CenteredLoader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div class="flex h-full w-full items-center justify-center text-ink">
            <Loader size=size />
        </div>
    }
}

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
