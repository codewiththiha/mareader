//! Generic <select> atom. The selected option is driven by `value` (a signal) and
//! changes flow back out through `on_change`. Options are a static list (the
//! zoom presets / textures / themes never change at runtime).

use leptos::prelude::*;

#[component]
pub fn Select<T>(
    options: Vec<(T, String)>,
    value: ReadSignal<T>,
    on_change: impl Fn(T) + 'static,
    #[prop(optional)] title: Option<String>,
) -> impl IntoView
where
    T: PartialEq + Clone + Send + Sync + 'static,
{
    // Three owned copies so the `each`, `children`, and `on:change` closures
    // each move their own.
    let each_opts = options.clone();
    let change_opts = options.clone();
    let children_opts = options;

    view! {
        <select
            title=title
            on:change=move |ev| {
                let v = event_target_value(&ev);
                if let Ok(idx) = v.parse::<usize>() {
                    if let Some((t, _)) = change_opts.get(idx) {
                        on_change(t.clone());
                    }
                }
            }
            class="h-9 rounded-lg border border-line bg-surface px-2 text-sm text-ink focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
        >
            <For
                each=move || each_opts.clone()
                key=|(_, label)| label.clone()
                children=move |(opt, label)| {
                    let idx = children_opts
                        .iter()
                        .position(|(t0, _)| t0 == &opt)
                        .unwrap_or(0);
                    let selected = value.get() == opt;
                    view! {
                        <option value=idx.to_string() prop:selected=selected>
                            {label}
                        </option>
                    }
                }
            />
        </select>
    }
}
