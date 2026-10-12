//! One dropdown: the rows it offers, and the one that stands.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};

use super::menu_item::MenuItem;
use crate::components::primitives::floating::menu_popover::MenuPopover;

/// The open menu's width, in CSS px.
const MENU_WIDTH: u32 = 200;

/// One dropdown over a list of named values.
#[component]
pub fn SelectMenu(
    /// The rows: a value and its label. `None` names no choice at all.
    #[prop(into)]
    options: Signal<Vec<(Option<String>, String)>>,
    /// The value that stands; a row taken away leaves the button blank.
    #[prop(into)]
    current: Signal<Option<String>>,
    on_pick: Callback<Option<String>>,
    /// `false` closes it: a rail panel hides with `display:none`,
    /// which orphans a popover left armed.
    #[prop(into, default = Signal::derive(|| true))]
    alive: Signal<bool>,
    /// Whether opening this menu holds the reader titlebar open.
    #[prop(default = false)]
    hold_titlebar: bool,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();
    // An offer taken away leaves the button blank, not lying.
    let label = move || {
        let want = current.get();
        options
            .get()
            .into_iter()
            .find(|(value, _)| *value == want)
            .map(|(_, text)| text)
            .unwrap_or_default()
    };

    Effect::new(move |_| {
        if !alive.get() {
            open.set(false);
        }
    });
    on_cleanup(move || {
        let _ = open.try_set(false);
    });

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                on:click=move |_| open.set(!open.get())
                class="flex max-w-[150px] items-center gap-1 rounded-md border border-line px-1.5 py-0.5 text-xs text-ink hover:bg-line focus:outline-none focus-visible:ring-2 focus-visible:ring-accent"
            >
                <span class="truncate">{label}</span>
                <Icon name=IconName::ChevronDown size=11 class="shrink-0 text-muted" />
            </button>
            <MenuPopover
                open=open
                anchor=root_ref
                width=MENU_WIDTH
                class="p-1".to_string()
                hold_titlebar=hold_titlebar
            >
                {move || {
                    options
                        .get()
                        .into_iter()
                        .map(|(value, text)| {
                            let picked = value.clone();
                            let click = value;
                            let is_current = Signal::derive(move || current.get() == picked);
                            view! {
                                <MenuItem
                                    label=text
                                    selected=is_current
                                    check=true
                                    on_click=move || {
                                        on_pick.run(click.clone());
                                        open.set(false);
                                    }
                                />
                            }
                        })
                        .collect_view()
                }}
            </MenuPopover>
        </div>
    }
}
