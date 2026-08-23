//! Generic segmented control.
//!
//! A compact pill row of two-or-more mutually exclusive options. The active
//! option is background-based (`bg-accent-soft text-accent`) per the mix-blend
//! rule — no `mix-blend-difference` / `text-white` on control states.
//!
//! Two display modes:
//! - compact icon-only (default) for the toolbar
//! - `full_width` for the overflow menu: container becomes `flex w-full` and
//!   every option is `flex-1`, so two options split the row 50/50 exactly
//!   regardless of label length.

use std::rc::Rc;

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};

/// What a segment displays: an icon glyph, or an icon + text label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentedLabel {
    Icon(IconName),
    IconText(IconName, &'static str),
}

/// One selectable segment: the value it produces, what it displays, and its
/// accessibility title.
pub struct SegmentOption<T> {
    pub value: T,
    pub label: SegmentedLabel,
    pub title: &'static str,
}

/// Segmented control.
///
/// The `title` is REQUIRED on every option because the labels are typically
/// icon-only: without it the buttons are anonymous to screen readers, hover
/// tooltips, and automated tests.
#[component]
pub fn Segmented<T: PartialEq + Copy + Send + Sync + 'static>(
    options: Vec<SegmentOption<T>>,
    value: ReadSignal<T>,
    on_change: impl Fn(T) + 'static,
    /// Stretch to the parent width and split it into perfect equal shares
    /// (overflow menu). Default stays compact icon-only for the toolbar.
    #[prop(default = false)]
    full_width: bool,
) -> impl IntoView {
    // `on_change` is called from one closure per option, so it rides in an Rc
    // that each button clones.
    let on_change = Rc::new(on_change);

    let container_class = if full_width {
        "flex w-full items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5"
    } else {
        "inline-flex items-center gap-0.5 rounded-lg border border-line bg-surface p-0.5"
    };

    view! {
        <div class=container_class>
            {options
                .into_iter()
                .map(move |opt| {
                    let t = opt.value;
                    let label = opt.label;
                    let title = opt.title;
                    let cb = Rc::clone(&on_change);
                    let class = move || {
                        let base = if full_width {
                            "flex h-8 min-w-0 flex-1 items-center justify-center gap-1.5 rounded-md px-2 text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        } else {
                            "inline-flex h-8 items-center justify-center rounded-md px-2.5 text-sm transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
                        };
                        if value.get() == t {
                            format!("{base} bg-accent-soft text-accent")
                        } else {
                            format!("{base} text-muted hover:text-ink")
                        }
                    };
                    let content = match label {
                        SegmentedLabel::Icon(i) => view! { <Icon name=i size=16 /> }.into_any(),
                        SegmentedLabel::IconText(i, text) => view! {
                            <>
                                <Icon name=i size=16 />
                                <span class="truncate">{text}</span>
                            </>
                        }.into_any(),
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
