//! Generic segmented control. OWNED BY U7 (phase 3): replaces two ToggleButtons.
//! Stub only — the body is filled by U7; it must simply compile with zero
//! warnings until then.

use leptos::prelude::*;

#[allow(dead_code)] // consumed in phase 3 (U7)
#[component]
pub fn Segmented<T: PartialEq + Copy + 'static>(
    options: Vec<(T, &'static str)>,
    value: ReadSignal<T>,
    on_change: impl Fn(T) + 'static,
) -> impl IntoView {
    // Params intentionally unused until U7 fills the body.
    let _ = (options, value, on_change);
    view! {}
}
