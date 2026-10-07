//! Minimal tooltip: children plus a native `title` attribute.

use leptos::prelude::*;

#[component]
pub fn Tooltip(#[prop(into)] text: String, children: Children) -> impl IntoView {
    view! {
        <span title=text class="inline-flex">{children()}</span>
    }
}
