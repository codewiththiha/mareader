//! The host's view: the workspace chrome and the slot the panes mount in.

use leptos::html;
use leptos::prelude::*;
use wasm_bindgen::JsCast;

use super::ReaderHost;
use super::contract::{ChromeSlot, PaneSite};
use super::manager::PaneManager;
use super::model::PaneId;
use super::tree::{PaneTree, SplitAxis, SplitId};
use crate::components::shell::sidebar::overlay::OverlayRail;
use crate::components::shell::sidebar::push::PushRail;
use app_chrome::hooks::dom::{TOOLBAR_LEADING_ID, VIEWER_SLOT_ID};
use app_chrome::icon::{Icon, IconName};
use app_chrome::layers;
use app_chrome::tooltip::Tooltip;
use app_ui::components::menus::appearance_menu::AppearanceMenu;
use app_ui::components::primitives::controls::button::{Button, ButtonVariant};
use app_ui::components::shell::titlebar::app_title_bar::AppTitleBar;

/// The title bar's height (`h-12`): it lies over the workspace top and
/// takes presses there.
const TITLE_BAR_PX: f64 = 48.0;

/// The active pane's contribution to `slot`: a focus change re-places
/// it.
fn slot_view(
    host: ReaderHost,
    slot: ChromeSlot,
) -> impl Fn() -> Option<AnyView> + Copy + Send + Sync + 'static {
    move || {
        let pane = host.manager.active_pane()?;
        let site = PaneSite::here();
        untrack(|| pane.chrome(slot, site))
    }
}

/// The workspace: the title bar, the backdrop, the rail mounts, the
/// pane slot.
#[component]
pub fn ReaderHostView(host: ReaderHost) -> impl IntoView {
    let shell = host.shell;
    let settings = host.session.settings;

    // Always mounted: the cluster anchors the library title; the toggle's
    // visibility is the controller's rule.
    let show_sidebar_toggle = move || shell.show_sidebar_toggle().get();
    let has_open_doc = move || host.active_status().holds_document();
    let go_library = move |_| host.return_to_library();
    let left = move || {
        view! {
            <div
                id=TOOLBAR_LEADING_ID
                data-tauri-drag-region="true"
                class="flex shrink-0 items-center gap-1"
            >
                <Show when=show_sidebar_toggle>
                    <Tooltip text="Toggle sidebar">
                        <Button
                            on_click=move |_| shell.toggle_sidebar()
                            variant=ButtonVariant::Ghost
                            title="Toggle sidebar"
                        >
                            <Icon name=IconName::Sidebar size=18 />
                        </Button>
                    </Tooltip>
                </Show>
                <Show when=has_open_doc>
                    <Tooltip text="Library">
                        <Button
                            on_click=go_library
                            variant=ButtonVariant::Ghost
                            title="Close this book and return to the library"
                        >
                            <Icon name=IconName::Library size=18 />
                        </Button>
                    </Tooltip>
                </Show>
            </div>
        }
    };
    let center = slot_view(host, ChromeSlot::TitleCenter);
    let trailing = slot_view(host, ChromeSlot::TitleTrailing);
    let right = move || {
        view! {
            {trailing}
            <AppearanceMenu state=host.chrome theme=host.theme_handle() />
        }
    };
    let rail_push = slot_view(host, ChromeSlot::Rail);
    let rail_overlay = slot_view(host, ChromeSlot::Rail);
    let settings_modal = slot_view(host, ChromeSlot::Settings);

    // One entry per placed pane, keyed by PANE id, at the box handed it.
    let manager = host.manager;
    let pane_entry = move |id: PaneId| {
        let Some(pane) = manager.pane(id) else {
            return ().into_any();
        };
        let site = PaneSite::here();
        let content = untrack(|| pane.mount(host.bounds_now(id), site));
        // The pane's effects are installed and its view built here (parked
        // off screen right away).
        host.pane_ready(id);
        let active = move || manager.active() == Some(id);
        let split = move || host.pane_count() > 1;
        // Focus is decided at the ENTRY, in the capture phase, before any
        // control sees it.
        let entry_ref: NodeRef<html::Div> = NodeRef::new();
        entry_ref.on_load(move |entry| {
            let entry: web_sys::Element = entry.into();
            capture_focus(&entry, manager, id);
        });
        let bounds = move || {
            manager
                .bounds_of(id)
                .filter(|b| b.width > 0.0 && b.height > 0.0)
        };
        // A pane whose top meets the title bar keeps its corner controls
        // below the bar.
        let under_bar = Signal::derive(move || bounds().is_none_or(|b| b.y < TITLE_BAR_PX));
        let corner = move || if under_bar.get() { "top-14" } else { "top-2" };
        // While lifted, the entry rides the pointer as a card, shrunk about
        // its pivot.
        let lifted = move || host.lifted().filter(|lift| lift.pane == id);
        let ride = move || {
            lifted()
                .map(|l| format!("{}px {}px", l.at.0 - l.origin.0, l.at.1 - l.origin.1))
                .unwrap_or_default()
        };
        let pivot = move || {
            lifted()
                .zip(bounds())
                .map(|(l, b)| format!("{}px {}px", l.origin.0 - b.x, l.origin.1 - b.y))
                .unwrap_or_default()
        };
        view! {
            <div
                node_ref=entry_ref
                class="pane-entry group absolute"
                class=("pane-lifted", move || lifted().is_some())
                // A closed pane retiring: out of sight, still in the document.
                class=("pane-retiring", move || host.is_retiring(id))
                style:translate=ride
                style:transform-origin=pivot
                style:left=move || bounds().map_or("0px".to_string(), |b| format!("{}px", b.x))
                style:top=move || bounds().map_or("0px".to_string(), |b| format!("{}px", b.y))
                style:width=move || {
                    bounds().map_or("100%".to_string(), |b| format!("{}px", b.width))
                }
                style:height=move || {
                    bounds().map_or("100%".to_string(), |b| format!("{}px", b.height))
                }
                data-pane-id=id.get()
                data-pane-active=move || active().to_string()
            >
                {content}
                // The focus outline: which pane the keyboard follows.
                <Show when=move || split() && active()>
                    <div
                        aria-hidden="true"
                        class="pane-focus-outline pointer-events-none absolute inset-0"
                    />
                </Show>
                // Close THIS pane; with one pane, Library closes all.
                <Show when=split>
                    <div
                        data-pane-close=id.get()
                        class=move || {
                            format!(
                                "absolute right-2 {} opacity-0 transition-opacity \
                                 group-hover:opacity-100 focus-within:opacity-100 {}",
                                corner(),
                                layers::CONTROLS,
                            )
                        }
                    >
                        <Button
                            on_click=move |ev| {
                                ev.stop_propagation();
                                if let Err(error) = host.close_pane(id) {
                                    leptos::logging::warn!("[reader] pane close refused: {error:?}");
                                }
                            }
                            variant=ButtonVariant::Ghost
                            title="Close this pane"
                            compact=true
                        >
                            <Icon name=IconName::Close size=14 />
                        </Button>
                    </div>
                </Show>
            </div>
        }
        .into_any()
    };

    // One divider per split, keyed by the split: strip and box come from
    // the layout.
    let divider = move |split: SplitId| divider_view(host, split);

    view! {
        <AppTitleBar state=host.chrome left=left center=center right=right>
            // overflow-hidden clips the bottom bar's slide-down, so no phantom
            // scrollbar.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("independent-themes", move || host.themes.active().get())
                class=("split-workspace", move || host.pane_count() > 1)
                style=move || host.workspace_look().style
                // The blend class swaps backdrop and page hosts onto the paper.
                class=("blend", move || host.workspace_look().blend)
                class=("pane-shield", move || host.shielded())
            >
                <div class="relative flex min-h-0 flex-1">
                    // DOCKED: the rail sits beside `<main>` and takes width.
                    <PushRail shell=shell>{rail_push}</PushRail>
                    <main
                        id=VIEWER_SLOT_ID
                        class="relative min-w-0 flex-1 overflow-hidden"
                        class=("no-page-shadow", move || !settings.with(|st| st.layout.page_shadow))
                    >
                        <For
                            each=move || host.entries()
                            key=|id| *id
                            children=pane_entry
                        />
                        <For
                            each=move || {
                                host.layout().dividers.iter().map(|d| d.split).collect::<Vec<_>>()
                            }
                            key=|split| *split
                            children=divider
                        />
                        {move || host.drag_preview().map(preview_view)}
                        {move || host.lifted().map(|lift| lift_view(manager, lift))}
                        // The pending drop in words, for screen readers.
                        <div class="sr-only" role="status" aria-live="polite" data-drop-announce="">
                            {move || host.drag_preview().map(|preview| preview.label).unwrap_or_default()}
                        </div>
                    </main>
                </div>
            </div>
            // OVERLAY: `OverlayRail` mounts outside `.reader-bg` (a stacking
            // context), so its z-popover outranks the title bar.
            <OverlayRail shell=shell>{rail_overlay}</OverlayRail>
            // The settings modal belongs to the window: inside `.reader-bg` the
            // bar would paint over it.
            {settings_modal}
        </AppTitleBar>
    }
}

/// The pending drop's box and words. Pure geometry; never opens
/// anything, never takes a pointer.
fn preview_view(preview: super::drag::Preview) -> impl IntoView {
    let rect = preview.rect;
    view! {
        <div
            aria-hidden="true"
            data-drop-preview=preview.target.word()
            data-drop-pane=preview.target.pane().get()
            class=format!(
                "pointer-events-none absolute flex items-center justify-center rounded-md \
                 border-2 border-accent bg-accent/15 text-sm font-medium text-ink \
                 motion-safe:transition-all motion-safe:duration-100 {}",
                layers::CONTROLS,
            )
            style:left=format!("{}px", rect.x)
            style:top=format!("{}px", rect.y)
            style:width=format!("{}px", rect.width)
            style:height=format!("{}px", rect.height)
        >
            <div class="flex max-w-[80%] flex-col items-center gap-0.5 rounded-md bg-surface/85 px-3 py-1.5 text-center shadow-sm">
                <span class="max-w-full truncate">{preview.name}</span>
                <span class="text-xs font-normal text-muted">{preview.label}</span>
            </div>
        </div>
    }
}

/// The lifted pane's mark: the box it would take if released now.
fn lift_view(manager: PaneManager, lift: super::lift::Lift) -> impl IntoView {
    let px = |v: f64| format!("{v}px");
    let target = lift.target.and_then(|target| {
        let rect = target.predicted_rect(manager.bounds_of(target.pane())?);
        Some((target, rect))
    });
    view! {
        {target.map(|(target, rect)| view! {
            <div
                aria-hidden="true"
                class=format!(
                    "pane-lift-target pointer-events-none absolute flex items-center \
                     justify-center {}",
                    layers::CONTROLS,
                )
                style:left=px(rect.x)
                style:top=px(rect.y)
                style:width=px(rect.width)
                style:height=px(rect.height)
            >
                <span class="pane-lift-label">{target.describe()}</span>
            </div>
        })}
        <div class="sr-only" role="status" aria-live="polite">
            {lift.target.map_or("Pane lifted", |target| target.describe())}
        </div>
    }
}

/// Make pane `id` active on any press or focus inside `entry`, in
/// the capture phase.
fn capture_focus(entry: &web_sys::Element, manager: PaneManager, id: PaneId) {
    let activate = wasm_bindgen::closure::Closure::<dyn Fn(web_sys::Event)>::new(
        move |event: web_sys::Event| {
            let on_close = event
                .target()
                .and_then(|target| target.dyn_into::<web_sys::Element>().ok())
                .and_then(|el| el.closest("[data-pane-close]").ok().flatten())
                .is_some();
            if !on_close {
                let _ = manager.set_active(id);
            }
        },
    )
    .into_js_value();
    for name in ["pointerdown", "focusin"] {
        let _ =
            entry.add_event_listener_with_callback_and_bool(name, activate.unchecked_ref(), true);
    }
}

/// One split's strip over the seam, painting nothing: a drag becomes
/// a ratio through [`PaneTree::drag_ratio`].
fn divider_view(host: ReaderHost, split: SplitId) -> impl IntoView {
    let current = move || {
        host.layout()
            .dividers
            .into_iter()
            .find(|d| d.split == split)
    };
    let current_untracked = move || untrack(current);
    let origin: StoredValue<Option<(f64, f64)>> = StoredValue::new(None);
    let axis = move || current().map_or(SplitAxis::Horizontal, |d| d.axis);
    let px = |v: f64| format!("{v}px");
    let hit = move || current().map(|d| d.hit).unwrap_or_default();
    view! {
        <div
            role="separator"
            data-split-id=split.get()
            aria-orientation=move || match axis() {
                SplitAxis::Horizontal => "vertical",
                SplitAxis::Vertical => "horizontal",
            }
            class=move || {
                format!(
                    "absolute touch-none select-none {} {}",
                    layers::CONTROLS,
                    match axis() {
                        SplitAxis::Horizontal => "cursor-col-resize",
                        SplitAxis::Vertical => "cursor-row-resize",
                    },
                )
            }
            style:left=move || px(hit().x)
            style:top=move || px(hit().y)
            style:width=move || px(hit().width)
            style:height=move || px(hit().height)
            on:pointerdown=move |ev| {
                if ev.button() != 0 {
                    return;
                }
                ev.prevent_default();
                let slot = web_sys::window()
                    .and_then(|w| w.document())
                    .and_then(|d| d.get_element_by_id(VIEWER_SLOT_ID));
                let Some(slot) = slot else {
                    return;
                };
                let rect = slot.get_bounding_client_rect();
                origin.set_value(Some((rect.left(), rect.top())));
                if let Some(target) = ev.current_target()
                    && let Ok(el) = target.dyn_into::<web_sys::Element>()
                {
                    let _ = el.set_pointer_capture(ev.pointer_id());
                }
                host.resizing.set(true);
            }
            on:pointermove=move |ev| {
                let (Some((left, top)), Some(divider)) = (origin.get_value(), current_untracked())
                else {
                    return;
                };
                let pointer = (ev.client_x() as f64 - left, ev.client_y() as f64 - top);
                if let Some(ratio) = PaneTree::drag_ratio(divider.axis, divider.span, pointer) {
                    host.drag_divider(split, ratio);
                }
            }
            on:pointerup=move |_| {
                origin.set_value(None);
                host.resizing.set(false);
            }
            on:pointercancel=move |_| {
                origin.set_value(None);
                host.resizing.set(false);
            }
        >
        </div>
    }
}
