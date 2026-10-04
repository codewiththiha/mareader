//! The host's view: the workspace chrome and the slot the panes mount in.
//!
//! The title bar, the backdrop, the rail's two mount points and the settings
//! modal's placement are the host's — ONE copy for the workspace, whatever
//! the panes show. Each chrome region the active pane may fill is a
//! [`ChromeSlot`] the host places and the pane fills through the contract;
//! the workspace slot (`main#viewer-slot`) holds one keyed entry per placed
//! pane, positioned at the box the layout gave it, and one divider per
//! split.
//!
//! The workspace-level overlays are the host's too, never a pane's: the
//! active pane's focus outline, each pane's close control, the dividers
//! (§22), and a drag's drop preview — a box drawn from the drag's measured
//! geometry, never a render and never an open. A pane's own overlays (its
//! find bar, its selection pill, its gloss menus) stay inside its content.

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

/// The title bar's height (its root's `h-12`, always hit-testable): it lies over
/// the top of the workspace and takes presses there.
const TITLE_BAR_PX: f64 = 48.0;

/// The active pane's contribution to `slot`, placed where this closure
/// runs. Tracked on the active pane only: a focus change re-places the
/// region; nothing inside the pane re-runs it.
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

/// The workspace: the title bar (sidebar toggle + Library on the left, the
/// active document's title in the centre, its view menu and the host's
/// appearance menu on the right), the backdrop, the rail's mount points, the
/// pane slot, and the settings modal.
#[component]
pub fn ReaderHostView(host: ReaderHost) -> impl IntoView {
    let shell = host.shell;
    let settings = host.session.settings;

    // The sidebar toggle's visibility is the controller's rule: overlay mode
    // drops it (the rail opens by brushing the window's left edge and closes
    // from its own header). The Library button stays put — the rail floats
    // above the bar and covers it while up. The cluster is always mounted so
    // the row keeps its left edge (and `#toolbar-leading`, the measurement
    // anchor the library title uses) wherever the mode puts it.
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

    // The workspace slot's entries: one per placed pane, keyed by PANE id
    // (never a document id or an index), each positioned at the box the host
    // handed that pane — filling the slot until the first measurement
    // lands. Every placed pane is SHOWN and live: inactive is not hidden
    // and not disposed, it is only not the pane the keyboard and the
    // title bar's chrome follow.
    let manager = host.manager;
    let pane_entry = move |id: PaneId| {
        let Some(pane) = manager.pane(id) else {
            return ().into_any();
        };
        let site = PaneSite::here();
        let content = untrack(|| pane.mount(host.bounds_now(id), site));
        // The pane's effects are installed and its view is built (and, off
        // screen, it is parked right away).
        host.pane_ready(id);
        let active = move || manager.active() == Some(id);
        let split = move || host.pane_count() > 1;
        // Focus is decided at the ENTRY, in the capture phase: a press or a
        // keyboard focus anywhere in the pane makes it active before any
        // control inside can swallow the event.
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
        // below the bar, which would otherwise take their presses.
        let under_bar = Signal::derive(move || bounds().is_none_or(|b| b.y < TITLE_BAR_PX));
        let corner = move || if under_bar.get() { "top-14" } else { "top-2" };
        // While lifted, the entry rides the pointer as a card: shrunk about
        // the point it was picked up by, offset by how far the pointer went.
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
                // A closed pane finishing its teardown: out of sight and out
                // of reach, but still in the document (see `entries`).
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
                data-pane-format=move || {
                    manager
                        .pane(id)
                        .map(|pane| format!("{:?}", pane.format()).to_lowercase())
                        .unwrap_or_default()
                }
            >
                {content}
                // The focus outline: which of several panes the keyboard
                // and the title bar follow. Painted over the content, never
                // taking a pointer.
                <Show when=move || split() && active()>
                    <div
                        aria-hidden="true"
                        class="pane-focus-outline pointer-events-none absolute inset-0"
                    />
                </Show>
                // Close THIS pane (with more than one: the last pane closes
                // with the reader, through the Library button).
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

    // One divider per split, keyed by the split: its strip and the box it
    // resizes are read from the layout, so a re-layout moves it in place.
    let divider = move |split: SplitId| divider_view(host, split);

    view! {
        <AppTitleBar state=host.chrome left=left center=center right=right>
            // overflow-hidden clips the hidden bottom bar's slide-down
            // translate so it can never leak a phantom scrollbar onto the
            // window.
            <div
                class="reader-bg relative flex h-full w-full flex-col overflow-hidden text-ink"
                class=("independent-themes", move || host.themes.active().get())
                class=("split-workspace", move || host.pane_count() > 1)
                style=move || host.workspace_look().style
                // The blend class swaps the backdrop AND the page hosts onto
                // the engine's one computed paper colour (see
                // `ReaderHost::workspace_look`).
                class=("blend", move || host.workspace_look().blend)
                class=("pane-shield", move || host.shielded())
            >
                <div class="relative flex min-h-0 flex-1">
                    // DOCKED: the rail is a flex sibling of `<main>`, so the
                    // workspace gives up the width. `PushRail` renders
                    // nothing while the controller says the layout is
                    // overlay.
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
                        // The pending drop in words, for assistive technology:
                        // the text changes only when the target does.
                        <div class="sr-only" role="status" aria-live="polite" data-drop-announce="">
                            {move || host.drag_preview().map(|preview| preview.label).unwrap_or_default()}
                        </div>
                    </main>
                </div>
            </div>
            // OVERLAY: `OverlayRail` mounts OUTSIDE `.reader-bg`, which is a
            // stacking context at z-index 0 — a rail inside it would paint
            // under the title bar's band. Out here its own z-popover outranks
            // the bar, so the rail covers the bar's left corner (Library
            // button included) and takes the traffic lights with it. Renders
            // nothing while the controller says the layout is docked.
            <OverlayRail shell=shell>{rail_overlay}</OverlayRail>
            // The settings modal belongs to the window, not to the workspace:
            // inside `.reader-bg` (a stacking context) the title bar's band
            // would paint over an open modal. As a sibling, its own z-popover
            // token outranks the bar, and rendering after the floating rail
            // wins their shared token too.
            {settings_modal}
        </AppTitleBar>
    }
}

/// The pending drop: the box the dropped document will occupy, and what
/// the drop does. Pure geometry — nothing is opened or rendered for it —
/// and it never takes a pointer. Reduced motion drops its glide.
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

/// The lifted pane's mark on the workspace: the box it would take if
/// released now, with what the release does. Its old place is not held
/// open — the workspace is laid out without it while it is held — so the
/// neighbours have already filled it. Geometry only, never taking a pointer.
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
                data-lift-target=target.word()
                data-lift-pane=target.pane().get()
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
        <div class="sr-only" role="status" aria-live="polite" data-lift-announce="">
            {lift.target.map_or("Pane lifted", |target| target.describe())}
        </div>
    }
}

/// Make pane `id` active on any press or keyboard focus inside `entry`, in
/// the CAPTURE phase (see the entry). A press on the pane's close control
/// is not a request to look at the pane it is closing, and is left alone.
///
/// The listener's closure is handed to the element (`into_js_value`): it
/// lives exactly as long as the entry does and holds only Copy handles (the
/// manager's arena keys), so a detached entry keeps nothing of the session.
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

/// One split's divider: a pointer strip over the seam (the panes tile the
/// slot; the strip overlays both edges) with a hairline in its middle.
///
/// A drag converts the pointer's position along the split's box into a
/// ratio ([`PaneTree::drag_ratio`], which keeps both sides usable) and
/// hands it to the host, which applies the last one per animation frame
/// ([`ReaderHost::drag_divider`]). Pointer capture keeps the drag on the
/// strip wherever the pointer goes; the slot's client origin is read once,
/// at the press.
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
            <div
                aria-hidden="true"
                class="pointer-events-none absolute bg-line"
                style=move || match axis() {
                    SplitAxis::Horizontal => "left:50%;top:0;bottom:0;width:1px",
                    SplitAxis::Vertical => "top:50%;left:0;right:0;height:1px",
                }
            />
        </div>
    }
}

// only the changed file was rewritten
