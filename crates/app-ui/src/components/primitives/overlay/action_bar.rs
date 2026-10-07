//! Floating action bar: a fixed pill for bulk selection workflows.

use leptos::prelude::*;

/// A floating selection action bar.
#[component]
pub fn ActionBar(
    visible: Signal<bool>,
    children: ChildrenFn,
    /// Extra classes on the pill (positioning, surface name…).
    #[prop(optional, into)]
    class: Option<String>,
    #[prop(default = "toolbar")] role: &'static str,
    #[prop(optional, into)] aria_label: Option<String>,
) -> impl IntoView {
    let base = format!(
        "fixed bottom-5 right-5 {} flex items-center gap-1 rounded-full border border-line \
         bg-surface py-1.5 pl-4 pr-1.5 shadow-[var(--gloss-shadow-float)]",
        app_chrome::layers::SELECTION_BAR
    );
    let pill_class = match class {
        Some(c) => format!("{base} {c}"),
        None => base,
    };
    // Static class and label, parked in StoredValues; only visibility is
    // reactive.
    let pill_class: StoredValue<String, LocalStorage> = StoredValue::new_local(pill_class);
    let aria_label: StoredValue<Option<String>, LocalStorage> = StoredValue::new_local(aria_label);

    view! {
        <Show when=move || visible.get()>
            <div
                class=move || pill_class.with_value(String::clone)
                role=role
                aria-label=move || aria_label.with_value(Clone::clone)
            >
                {children()}
            </div>
        </Show>
    }
}
