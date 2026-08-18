//! Generic segmented control. OWNED BY U7 (phase 3): replaces two ToggleButtons.
//!
//! A compact pill row of two-or-more mutually exclusive options. The active
//! option is background-based (`bg-accent-soft text-accent`) per the mix-blend
//! rule — no `mix-blend-difference` / `text-white` on control states.

use std::rc::Rc;

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};

/// What a segment displays: an icon glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentedLabel {
    Icon(IconName),
}

/// Segmented control.
///
/// Each option is `(value, label, title)`. The `title` is REQUIRED because the
/// labels are typically icon-only: without it the buttons are anonymous to
/// screen readers, hover tooltips, and automated tests. The wrapping `Tooltip`
/// atom titles the group as a whole, not the individual segments.
#[component]
pub fn Segmented<T: PartialEq + Copy + Send + Sync + 'static>(
    options: Vec<(T, SegmentedLabel, &'static str)>,
    value: ReadSignal<T>,
    on_change: impl Fn(T) + 'static,
) -> impl IntoView {
    // `on_change` is called from one closure per option, so it rides in an Rc
    // that each button clones.
    let on_change = Rc::new(on_change);

    view! {
        <div class="inline-flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5">
            {options
                .into_iter()
                .map(move |(t, label, title)| {
                    let cb = Rc::clone(&on_change);
                    let class = move || {
                        let base = "inline-flex h-8 items-center justify-center rounded-md px-2.5 text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
                        if value.get() == t {
                            format!("{base} bg-accent-soft text-accent")
                        } else {
                            format!("{base} text-muted hover:text-ink")
                        }
                    };
                    let content = match label {
                        SegmentedLabel::Icon(i) => view! { <Icon name=i size=16 /> }.into_any(),
                    };
                    view! {
                        <button
                            type="button"
                            class=class
                            title=title
                            aria-label=title
                            aria-pressed=move || (value.get() == t).to_string()
                            on:click=move |_| cb(t)
                        >
                            {content}
                        </button>
                    }
                })
                .collect_view()}
        </div>
    }
}
