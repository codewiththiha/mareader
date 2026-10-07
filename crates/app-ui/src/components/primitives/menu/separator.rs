//! Thin divider; it owns its own margin, so no caller wraps it.

use leptos::prelude::*;

#[component]
pub fn Separator(
    #[prop(default = false)] vertical: bool,
    /// The divider's own outer spacing, as a static utility class.
    #[prop(optional, into)]
    spacing: Option<String>,
) -> impl IntoView {
    let spacing = spacing.unwrap_or_default();
    if vertical {
        let class = format!("mx-1 h-6 w-px shrink-0 bg-line {spacing}");
        view! { <div class=class /> }
    } else {
        let class = format!("h-px w-full shrink-0 bg-line {spacing}");
        view! { <div class=class /> }
    }
}
