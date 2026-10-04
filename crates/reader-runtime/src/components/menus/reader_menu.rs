//! Readest-style 3-dash reader menu: zoom, view modes, fit, auto-scroll, and tools.

use leptos::html;
use leptos::prelude::*;

use reader_core::view::ViewMode;
use reader_core::zoom_math::FitMode;

use crate::host::contract::{OpenRequest, Placement};
use crate::host::tree::{MoveDirection, SplitAxis};
use app_chrome::icon::{Icon, IconName};
use app_chrome::icon_button::IconButton;
use app_ui::components::primitives::floating::menu_popover::MenuPopover;
use app_ui::components::primitives::menu::kbd::Kbd;
use app_ui::components::primitives::menu::menu_item::MenuItem;
use app_ui::components::primitives::menu::separator::Separator;
use app_ui::components::primitives::menu::shortcut_row::ShortcutRow;

#[component]
fn ModeButton(
    state: crate::context::ReaderContext,
    m: ViewMode,
    icon: IconName,
    title: &'static str,
) -> impl IntoView {
    let pressed = Signal::derive(move || state.reader.viewer.mode.get() == m);
    view! {
        <IconButton
            icon=icon
            title=title
            pressed=pressed
            on_click=move || state.reader.viewer.mode.set(m)
        />
    }
}

#[component]
fn FitButton(
    state: crate::context::ReaderContext,
    f: FitMode,
    icon: IconName,
    title: &'static str,
) -> impl IntoView {
    let pressed = Signal::derive(move || state.reader.viewer.fit.get() == f);
    view! {
        <IconButton
            icon=icon
            title=title
            pressed=pressed
            on_click=move || state.reader.viewer.fit.set(f)
        />
    }
}

/// The Move item for `direction`: its arrow and its words.
fn move_item(direction: MoveDirection) -> (IconName, &'static str) {
    match direction {
        MoveDirection::Left => (IconName::MoveLeft, "Move Left"),
        MoveDirection::Right => (IconName::MoveRight, "Move Right"),
        MoveDirection::Up => (IconName::MoveUp, "Move Up"),
        MoveDirection::Down => (IconName::MoveDown, "Move Down"),
    }
}

/// Show this pane's document again in a new pane beside it, at the page
/// this pane is on. The host places it (and refuses it, with a toast, when
/// the workspace is full); this pane keeps its own session untouched.
fn split_beside(state: crate::context::ReaderContext, axis: SplitAxis) {
    let Some(launch) = crate::pane::document::view_again(state) else {
        return;
    };
    state.open.try_run(OpenRequest {
        launch,
        placement: Placement::Beside(axis),
    });
}

#[component]
pub fn ReaderMenu(
    state: crate::context::ReaderContext,
    settings_open: RwSignal<bool>,
) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();
    let r = state.reader;
    let mode = r.viewer.mode;
    let percent = move || format!("{}%", (r.viewer.zoom.display.get() * 100.0).round() as u32);
    let (show_keys, set_show_keys) = signal(false);

    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <IconButton
                icon=IconName::Dashes
                title="View & tools"
                on_click=move || open.set(!open.get())
            />
            <MenuPopover
                open=open
                anchor=root_ref
                width=300u32
                coordinate_space="toolbar-row"
                class="p-2".to_string()
            >
                <div class="flex items-center justify-between px-2 py-1">
                    <IconButton
                        icon=IconName::ZoomOut
                        title="Zoom out (-)"
                        on_click=move || state.reader.viewer.ask_zoom_step(-1)
                    />
                    <span class="text-sm font-medium tabular-nums text-ink">{percent}</span>
                    <IconButton
                        icon=IconName::ZoomIn
                        title="Zoom in (+)"
                        on_click=move || state.reader.viewer.ask_zoom_step(1)
                    />
                </div>
                <div class="flex items-center justify-center gap-1 px-2 py-1">
                    <ModeButton state=state m=ViewMode::Single icon=IconName::SinglePage title="Single page" />
                    <ModeButton state=state m=ViewMode::Spread icon=IconName::DualPage title="Two pages" />
                    <ModeButton state=state m=ViewMode::ScrollVertical icon=IconName::Continuous title="Vertical scroll" />
                    <ModeButton state=state m=ViewMode::ScrollHorizontal icon=IconName::HScroll title="Horizontal scroll" />
                    <div class="mx-1 h-6 w-px shrink-0 bg-line"></div>
                    <FitButton state=state f=FitMode::Width icon=IconName::FitWidth title="Fit width" />
                    <FitButton state=state f=FitMode::Page icon=IconName::FitPage title="Fit page" />
                </div>
                <Separator vertical=false spacing="my-1" />
                // ── Auto scroll: disabled + dimmed on paginated modes ──
                {move || {
                    let disabled = !mode.get().can_scroll();
                    view! {
                        <MenuItem
                            icon=IconName::AutoScroll
                            label="Auto Scroll".to_string()
                            disabled=disabled
                            selected=Signal::derive(move || r.viewer.auto_scroll.get())
                            on_click=move || r.viewer.auto_scroll.update(|v| *v = !*v)
                        >
                            <span class="ml-auto flex gap-0.5"><Kbd>"Shift"</Kbd><Kbd>"A"</Kbd></span>
                        </MenuItem>
                    }
                }}
                <Separator vertical=false spacing="my-1" />
                // ── The workspace: this document again beside itself, or
                // (desktop, where there is a file dialog) another one ──
                {move || {
                    let full = !state.can_split.get();
                    let empty = state.launch.with(|launch| launch.path.is_empty());
                    let moves = state.moves.get();
                    // In a split the menu moves THIS pane through the layout
                    // (the drop-free way to rearrange); with one pane there is
                    // nothing to move, so it offers the splits instead.
                    let placement = if moves.any() {
                        MoveDirection::ALL
                            .into_iter()
                            .map(|direction| {
                                let (icon, label) = move_item(direction);
                                view! {
                                    <MenuItem
                                        icon=icon
                                        label=label.to_string()
                                        disabled=!moves.allows(direction)
                                        on_click=move || {
                                            open.set(false);
                                            state.relocate.run(direction);
                                        }
                                    />
                                }
                            })
                            .collect_view()
                            .into_any()
                    } else {
                        view! {
                            <MenuItem
                                icon=IconName::SplitRight
                                label="Split Right".to_string()
                                disabled=full || empty
                                on_click=move || {
                                    open.set(false);
                                    split_beside(state, SplitAxis::Horizontal);
                                }
                            />
                            <MenuItem
                                icon=IconName::SplitDown
                                label="Split Down".to_string()
                                disabled=full || empty
                                on_click=move || {
                                    open.set(false);
                                    split_beside(state, SplitAxis::Vertical);
                                }
                            />
                        }
                        .into_any()
                    };
                    view! {
                        {placement}
                        {tauri_bridge::has_tauri().then(|| view! {
                            <MenuItem
                                icon=IconName::Open
                                label="Open Beside…".to_string()
                                disabled=full
                                on_click=move || {
                                    open.set(false);
                                    crate::services::document::open_dialog(
                                        state,
                                        Placement::Beside(SplitAxis::Horizontal),
                                    );
                                }
                            />
                        })}
                    }
                }}
                <Separator vertical=false spacing="my-1" />
                <MenuItem
                    icon=IconName::Settings
                    label="Settings…".to_string()
                    on_click=move || { open.set(false); settings_open.set(true); }
                />
                <Separator vertical=false spacing="my-1" />
                <MenuItem
                    icon=IconName::Keyboard
                    label="Keyboard Shortcuts".to_string()
                    on_click=move || set_show_keys.update(|v| *v = !*v)
                >
                    <Icon name=IconName::ChevronDown size=12 class="ml-auto text-muted" />
                </MenuItem>
                <Show when=move || show_keys.get()>
                    <div class="mt-1 max-h-56 overflow-y-auto border-t border-line pt-1">
                        <ShortcutRow label="Search…" keys=vec!["⌘", "F"] />
                        <ShortcutRow label="Auto scroll" keys=vec!["Shift", "A"] />
                        <ShortcutRow label="Prev / Next page" keys=vec!["←", "→"] />
                        <ShortcutRow label="Scroll (vim)" keys=vec!["H", "J", "K", "L"] />
                        <ShortcutRow label="Zoom" keys=vec!["+", "-"] />
                        <ShortcutRow label="Zoom (⌘ / Ctrl)" keys=vec!["+", "-"] />
                        <ShortcutRow label="Dismiss" keys=vec!["Esc"] />
                    </div>
                </Show>
                <Separator vertical=false spacing="my-1" />
                // The footprint escape hatch: the memory a long session
                // latches onto is the webview's, and no call the app can
                // make gives it back while the page lives — so the honest
                // reset is a reload, offered rather than imposed
                // (Mareader.md, "The memory model").
                <MenuItem
                    icon=IconName::Reload
                    label="Reload Window".to_string()
                    sublabel="Restarts in place; your place is kept, the memory is not".to_string()
                    on_click=move || {
                        open.set(false);
                        crate::services::reload::reload_app(state);
                    }
                />
                <div class="mt-1 flex items-center justify-between border-t border-line px-1 py-1">
                    <span class="text-xs text-muted">"Mareader"</span>
                </div>
            </MenuPopover>
        </div>
    }
}
