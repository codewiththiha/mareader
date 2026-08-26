//! Floating action bar: a fixed pill (bottom-right by default) for bulk
//! selection workflows — count readout, contextual actions, done/dismiss.
//! The gloss selection bar is its first consumer; annotation selection,
//! library item selection and future bulk ops can reuse it.

use leptos::prelude::*;

/// A floating selection action bar.
#[component]
pub fn ActionBar(
    /// Whether the bar is visible.
    visible: Signal<bool>,
    children: ChildrenFn,
    /// Extra classes on the pill (positioning, surface name…).
    #[prop(optional, into)]
    class: Option<String>,
    #[prop(default = "toolbar")]
    role: &'static str,
    #[prop(optional, into)]
    aria_label: Option<String>,
) -> impl IntoView {
    let base = format!(
        "fixed bottom-5 right-5 {} flex items-center gap-1 rounded-full border border-line \
         bg-surface py-1.5 pl-4 pr-1.5 shadow-[var(--gloss-shadow-menu)]",
        crate::components::primitives::floating::types::z::SELECTION_BAR
    );
    let pill_class = match class {
        Some(c) => format!("{base} {c}"),
        None => base,
    };
    // Static class + label for the bar's lifetime, parked in StoredValues —
    // Copy handles to plain scoped cells. `Show`'s children closure must be
    // an `Fn` and the attribute closures are moved into the element by
    // value, so the captured handles have to be Copy; signals would compile
    // too but would pretend static strings are reactive. Only visibility is
    // actually reactive here.
    let pill_class: StoredValue<String, LocalStorage> = StoredValue::new_local(pill_class);
    let aria_label: StoredValue<Option<String>, LocalStorage> =
        StoredValue::new_local(aria_label);

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
