//! Thin divider.
//!
//! The divider owns its own margin, because at all nineteen of its call sites it
//! was wrapped in a `<div>` that existed only to carry one — and four different
//! margins had grown between them, which is what happens when the spacing is
//! every caller's business rather than the rule's.

use leptos::prelude::*;

#[component]
pub fn Separator(
    #[prop(default = false)] vertical: bool,
    /// The divider's own outer spacing, as a static utility class — `"my-1"`,
    /// `"mt-5"`. A literal at the call site rather than a number, so the token is
    /// one Tailwind can see: the stylesheet is built by scanning the source for
    /// class names, and a margin computed at runtime is a margin that does not
    /// exist.
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
