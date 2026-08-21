//! Minimal tooltip: wraps children and exposes the text via a native `title`
//! attribute. Native tooltips are the most reliable cross-webview option and
//! avoid positioning bugs.

use leptos::prelude::*;

#[component]
pub fn Tooltip(
    text: String,
    children: Children,
) -> impl IntoView {
    view! {
        <span title=text class="inline-flex">{children()}</span>
    }
}
