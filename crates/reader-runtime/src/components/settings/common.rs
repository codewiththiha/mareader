//! Shared pieces of the settings modal: the tabs and the dropdown.

use leptos::html;
use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use app_ui::components::primitives::floating::menu_popover::MenuPopover;
use app_ui::components::primitives::form::row::Row;
use app_ui::components::primitives::menu::menu_item::MenuItem;
use app_ui::components::primitives::overlay::lanes::OverlayPolicy;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Layout,
    Theme,
    /// Hosted only while `Settings::animations.enabled` is on (see `modal`).
    Animations,
    /// Hosted only while a reflowable document is open.
    Fonts,
    /// The vocabulary highlighter: level slider and dataset.
    Vocabulary,
    /// The dictionary: hover, packs, languages.
    Dictionary,
    /// The split workspace: what a Library panel click does.
    Workspace,
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
            // An inactive tab is its icon alone; the name is its label.
            aria-label=label
            title=label
            class=class
        >
            <Icon name=icon size=17 />
            {move || (active.get() == t).then(|| view! { <span>{label}</span> })}
        </button>
    }
}

/// A −/+ adjuster row: value left, steppers right.
#[component]
pub(crate) fn StepperRow(
    label: &'static str,
    /// The formatted current value ("17 px", "1.7×", …).
    display: Signal<String>,
    #[prop(into)] minus_disabled: Signal<bool>,
    #[prop(into)] plus_disabled: Signal<bool>,
    on_minus: Callback<()>,
    on_plus: Callback<()>,
    /// What the steppers adjust; their tooltips derive from it.
    #[prop(into)]
    title: String,
) -> impl IntoView {
    let minus_title = format!("Decrease {title}");
    let plus_title = format!("Increase {title}");
    view! {
        <Row label=label>
            <span class="flex items-center gap-3">
                // Inert when neither stepper can move.
                <span
                    class="w-14 text-right text-sm tabular-nums text-ink"
                    class=("opacity-45", move || minus_disabled.get() && plus_disabled.get())
                >
                    {move || display.get()}
                </span>
                <span class="flex gap-1.5">
                    <IconButton
                        icon=IconName::Minus
                        size=14
                        title=minus_title
                        class="rounded-full bg-line/60 hover:bg-line".to_string()
                        disabled=minus_disabled
                        on_click=move || on_minus.run(())
                    />
                    <IconButton
                        icon=IconName::Plus
                        size=14
                        title=plus_title
                        class="rounded-full bg-line/60 hover:bg-line".to_string()
                        disabled=plus_disabled
                        on_click=move || on_plus.run(())
                    />
                </span>
            </span>
        </Row>
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
                // A dropdown here uses the in-dialog menu policy,
                // or it would evict the modal.
                policy=OverlayPolicy::IN_DIALOG
                // No reader title bar here to hold open.
                hold_titlebar=false
            >
                {opts.with_value(|opts| {
                    opts.iter()
                        .map(|(v, l)| {
                            // One option value, three closures.
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
