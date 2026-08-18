//! Minimal tooltip: wraps children and exposes the text via a native `title`
//! attribute. Native tooltips are the most reliable cross-webview option and
//! avoid positioning bugs.

use leptos::prelude::*;

#[component]
pub fn Tooltip(
    text: String,
    #[prop(default = "bottom")] side: &'static str,
    children: Children,
) -> impl IntoView {
    let _ = side; // reserved for future styled tooltip implementation
    view! {
        <span title=text class="inline-flex">{children()}</span>
    }
}
