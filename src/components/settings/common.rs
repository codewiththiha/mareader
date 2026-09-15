//! Shared pieces of the reader settings modal: the tab switcher and the generic
//! `StyleSelect` dropdown. The tabs (in their own files) build on these; `modal`
//! is the shell that hosts them.
//!
//! The labelled `Row` these sit beside is
//! [`crate::components::primitives::form::row::Row`]: the library's import
//! sheet and removal receipt are built out of the same rows, and a component two
//! features reach into a third for is a primitive with the wrong address.
//! `StyleSelect` stays because it is built on the toolbar's `MenuPopover`, and
//! moving it would make `primitives` depend on `shell` — the wrong way round.
//!
//! `TabButton` takes the tab to display as a SEPARATE signal from the one it
//! writes, because the tab set is not fixed: the Animations tab only exists
//! while its master switch is on, and the shell resolves that in a derived read
//! so nothing has to overwrite what the reader selected.

use leptos::html;
use leptos::prelude::*;

use crate::components::primitives::floating::menu_popover::MenuPopover;
use app_chrome::icon::{Icon, IconName};
use crate::components::primitives::menu::menu_item::MenuItem;
use crate::components::primitives::overlay::lanes::OverlayPolicy;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Layout,
    Theme,
    /// Hosted only while `Settings::animations.enabled` is on (see `modal`).
    Animations,
    /// Hosted only while a reflowable (TXT/Markdown) document is open —
    /// PDFs carry none of the type it controls (see `modal`).
    Fonts,
}

#[component]
pub(crate) fn TabButton(
    tab: RwSignal<Tab>,
    active: Signal<Tab>,
    t: Tab,
    icon: IconName,
    label: &'static str,
) -> impl IntoView {
    let class = move || {
        let base = "flex h-9 items-center justify-center rounded-lg transition-all \
focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
        if active.get() == t {
            format!("{base} gap-2 bg-accent-soft px-4 text-sm font-medium text-accent")
        } else {
            format!("{base} w-9 text-muted hover:text-ink")
        }
    };
    view! {
        <button
            type="button"
            on:click=move |_| tab.set(t)
            aria-pressed=move || (active.get() == t).to_string()
            class=class
        >
            <Icon name=icon size=17 />
            {move || (active.get() == t).then(|| view! { <span>{label}</span> })}
        </button>
    }
}

#[component]
pub(crate) fn StyleSelect<T>(
    value: Signal<T>,
    on_change: Callback<T>,
    options: Vec<(T, &'static str)>,
    label_of: fn(&T) -> &'static str,
    disabled: Signal<bool>,
) -> impl IntoView
where
    // Clone, not Copy: the Fonts tab offers `FontChoice`, whose built-in
    // variant carries a `String`.
    T: Clone + PartialEq + Send + Sync + 'static,
{
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();
    let opts = StoredValue::new(options);
    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                prop:disabled=move || disabled.get()
                on:click=move |_| open.set(!open.get())
                class="flex items-center gap-1.5 rounded-md px-2 py-1 text-sm text-ink \
hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent \
disabled:cursor-not-allowed disabled:opacity-45"
            >
                <span>{move || label_of(&value.get())}</span>
                <Icon name=IconName::ChevronDown size=12 class="text-muted" />
            </button>
            <MenuPopover
                open=open
                anchor=root_ref
                width=190u32
                class="p-1".to_string()
                // A dropdown INSIDE the settings modal is part of the dialog,
                // not a competitor for the window: the default MENU policy
                // would evict the modal the frame the list opens. The
                // in-dialog policy owns no lane and clears none, so the modal
                // stays put while the list is up (an outside press — clicking
                // anywhere else in the dialog — still closes the list).
                policy=OverlayPolicy::IN_DIALOG
                // Nothing here sits under the reader title bar, so there is
                // no bar to hold open while the list is up.
                hold_titlebar=false
            >
                {opts.with_value(|opts| {
                    opts.iter()
                        .map(|(v, l)| {
                            // One option value, three closures (the selected
                            // memo, the click handler, the check glyph): each
                            // takes its own clone now that T is no longer Copy.
                            let v_selected = v.clone();
                            let v_for_click = v.clone();
                            let label = (*l).to_string();
                            view! {
                                <MenuItem
                                    label=label
                                    selected=Signal::derive(move || value.get() == v_selected)
                                    check=true
                                    on_click=move || {
                                        on_change.run(v_for_click.clone());
                                        open.set(false);
                                    }
                                />
                            }
                        })
                        .collect_view()
                })}
            </MenuPopover>
        </div>
    }
}
