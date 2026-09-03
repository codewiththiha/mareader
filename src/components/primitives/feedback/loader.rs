//! The app's one loading mark: three dots chasing around a square.
//!
//! Pure CSS (`.loader` in `styles/components/animations.css`), painted in
//! `currentColor` so it takes whatever ink the surface around it uses — a
//! dark theme, a tinted paper, a muted caption all colour it for free. It
//! carries no timing of its own beyond the loop: the caller mounts it while
//! something is loading and unmounts it the moment that thing exists, so it
//! is never on screen a frame longer than the wait it stands for.

use leptos::prelude::*;

/// Default edge of the mark in CSS px. Big enough to read from across the
/// window when it sits alone in the middle of a surface.
const DEFAULT_SIZE: u32 = 72;

/// The three-dot loader. `size` is the square's edge in CSS px.
#[component]
pub fn Loader(#[prop(default = DEFAULT_SIZE)] size: u32) -> impl IntoView {
    view! {
        <div
            class="loader"
            role="status"
            aria-label="Loading"
            style=format!("width:{size}px")
        ></div>
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
