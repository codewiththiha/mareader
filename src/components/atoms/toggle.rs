//! Switch/toggle atom bound to a boolean signal.

use leptos::prelude::*;

#[component]
pub fn Toggle(
    checked: ReadSignal<bool>,
    on_change: impl Fn(bool) + 'static,
    #[prop(optional)] label: Option<String>,
) -> impl IntoView {
    let track = move || if checked.get() { "bg-accent" } else { "bg-line" };
    let knob = move || if checked.get() { "translate-x-4" } else { "translate-x-0.5" };

    view! {
        <button
            type="button"
            role="switch"
            aria-checked=move || checked.get().to_string()
            on:click=move |_| on_change(!checked.get())
            class="inline-flex items-center gap-2 focus:outline-none"
        >
            <span
                class=move || format!(
                    "relative inline-flex h-5 w-9 shrink-0 items-center rounded-full transition-colors {}",
                    track()
                )
            >
                <span
                    class=move || format!(
                        "inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform {}",
                        knob()
                    )
                />
            </span>
            {label.map(|l| view! { <span class="text-sm text-ink">{l}</span> })}
        </button>
    }
}
