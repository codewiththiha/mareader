//! The ellipsis and its panel: the affordance standing for the levels the
//! bar has folded, the hover intent that opens it one beat behind the
//! pointer, and the chain inside packed into rows the window's width decides.

use std::time::Duration;

use leptos::html;
use leptos::prelude::*;

use app_chrome::hooks::dom::by_id;
use app_chrome::icon::{Icon, IconName};

use crate::features::library::dnd::controller::DragController;
use crate::features::library::dnd::target::{DropTargetEntry, DropTargetId, DropTargetKind};
use app_ui::components::primitives::floating::menu_popover::MenuPopover;

use super::fold::{pack_rows, row_widths, split_by_counts};
use super::{Crumb, register_crumb};

/// Deliberately not in the drag's registry: it stands for no level, and a
/// measurement target would be a way to file books onto a ruler.
const ELIDED_MAX_DOM_ID: &str = "crumb-elided-max";

/// The floor keeps a sliver of a window from producing a budget no crumb can
/// be laid into; a crumb wider than the full budget gets a row to itself.
fn elided_budget_px() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .map(|window| (window - 24.0).max(160.0))
        .unwrap_or(0.0)
}

/// Deliberately not a `crumb-` id: sharing the crumbs' scheme would let a
/// reader of the registry mistake it for one.
const ELLIPSIS_DOM_ID: &str = "crumb-elided";

const MENU_CLOSE_GRACE_MS: u64 = 220;

/// The close is a beat behind the leave, owned by an effect on `over` rather
/// than a parked timer: exactly one timer, cancelled by the same thing that
/// arms it.
#[derive(Clone, Copy)]
pub(super) struct HoverIntent {
    open: RwSignal<bool>,
    over: RwSignal<bool>,
}

impl HoverIntent {
    pub(super) fn new() -> Self {
        let this = Self {
            open: RwSignal::new(false),
            over: RwSignal::new(false),
        };
        Effect::new(move |_| {
            if this.over.get() {
                return;
            }
            let open = this.open;
            let Ok(close) = set_timeout_with_handle(
                move || {
                    // A grace timer can outlive the panel's owner (the bar
                    // closed mid-hover): a disposed signal means the menu went
                    // with it, so the close is already done.
                    let _ = open.try_set(false);
                },
                Duration::from_millis(MENU_CLOSE_GRACE_MS),
            ) else {
                return;
            };
            on_cleanup(move || close.clear());
        });
        this
    }

    pub(super) fn enter(&self) {
        self.over.set(true);
        self.open.set(true);
    }

    pub(super) fn leave(&self) {
        self.over.set(false);
    }

    pub(super) fn close(&self) {
        self.over.set(false);
        self.open.set(false);
    }
}

/// Not an arrow on a crumb (see the module docs for the confusion an arrow
/// makes). A button, so a keyboard reaches the panel the way a pointer does.
#[component]
pub(super) fn EllipsisCrumb(
    state: crate::context::LibraryContext,
    ctrl: DragController,
    elided: Vec<Crumb>,
    intent: HoverIntent,
) -> impl IntoView {
    ctrl.registry.register(DropTargetEntry {
        id: DropTargetId(DropTargetKind::Ellipsis, String::new()),
        dom_id: ELLIPSIS_DOM_ID.to_string(),
        shelf: None,
    });
    let anchor: NodeRef<html::Div> = NodeRef::new();
    let live = ctrl.live();
    let tooltip = format!("Show the {} levels above", elided.len());
    let aria = tooltip.clone();
    // A component's children are an `Fn`; a children closure that owned the
    // list would be an `FnOnce` after building one crumb.
    let folded: StoredValue<Vec<Crumb>, LocalStorage> = StoredValue::new_local(elided);
    let rows: RwSignal<Vec<Vec<Crumb>>> = RwSignal::new(vec![folded.get_value()]);

    // The panel's width is measured rather than picked, and its chain is
    // packed rather than wrapped: rows render as surfaces of their own, so a
    // short second row is a short rectangle, not a wide empty one.
    let panel_width: RwSignal<f64> = RwSignal::new(0.0);
    let measure = move || {
        let Some(ruler) = by_id(ELIDED_MAX_DOM_ID) else {
            return;
        };
        let budget = elided_budget_px();
        if budget <= 0.0 {
            return;
        }
        let widths = super::measure_children_widths(&ruler);
        if widths.is_empty() {
            return;
        }
        let counts = pack_rows(&widths, budget);
        let wide = row_widths(&widths, &counts).into_iter().fold(0.0, f64::max);
        if (panel_width.get_untracked() - wide).abs() > 0.5 {
            panel_width.set(wide);
        }
        let repacked = rows.with_untracked(|current| {
            current.len() != counts.len()
                || current
                    .iter()
                    .zip(&counts)
                    .any(|(row, count)| row.len() != *count)
        });
        if repacked {
            rows.set(split_by_counts(folded.get_value(), &counts));
        }
    };
    // Measured once the panel and its ruler have mounted, re-measured on
    // every resize while open: the width only moves when the chain or the
    // window does.
    Effect::new(move |_| {
        if !intent.open.get() {
            return;
        }
        request_animation_frame(measure);
        let handle = window_event_listener(leptos::ev::resize, move |_| measure());
        on_cleanup(move || handle.remove());
    });

    // A drag cannot raise a `mouseenter` (the pressed card holds the pointer
    // capture), so while a drag is live the panel opens from the session's
    // hot target.
    Effect::new(move |_| {
        if live.get() && ctrl.over_ellipsis() {
            intent.enter();
        }
    });

    view! {
        <div
            node_ref=anchor
            class="relative flex shrink-0 items-center"
            on:mouseenter=move |_| intent.enter()
            on:mouseleave=move |_| intent.leave()
        >
            <Icon name=IconName::Next size=13 class="shrink-0 text-muted" />
            <button
                id=ELLIPSIS_DOM_ID
                type="button"
                title=tooltip
                aria-label=aria
                aria-expanded=move || intent.open.get().to_string()
                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "ArrowDown" {
                        ev.prevent_default();
                        intent.enter();
                    }
                }
                class="flex shrink-0 items-center rounded-md px-1 py-0.5 text-muted \
                       transition-colors hover:bg-line hover:text-ink focus:outline-none \
                       focus-visible:ring-2 focus-visible:ring-accent"
            >
                <Icon name=IconName::More size=14 />
            </button>
            // The chain is built inside the popover's reactive child: each
            // crumb registers a drop target that must leave the registry when
            // the crumb unmounts.
            <MenuPopover
                open=intent.open
                anchor=anchor
                width=Signal::derive(move || panel_width.get() as u32)
                coordinate_space="toolbar-row"
                class="lib-elided-panel max-h-80 overflow-y-auto".to_string()
            >
                // The ruler wears the crumbs' own metrics; the pack reads
                // its boxes, one width per crumb.
                <div id=ELIDED_MAX_DOM_ID class="lib-elided-probe" aria-hidden="true">
                    {move || {
                        let levels = folded.get_value();
                        let last = levels.len().saturating_sub(1);
                        levels
                            .into_iter()
                            .enumerate()
                            .map(|(at, crumb)| {
                                view! {
                                    <span class="lib-elided-crumb">
                                        <span class="lib-elided-probe-label">
                                            <span class="truncate">{crumb.name}</span>
                                        </span>
                                        {(at != last).then(|| {
                                            view! {
                                                <Icon name=IconName::Next size=13 class="shrink-0" />
                                            }
                                        })}
                                    </span>
                                }
                            })
                            .collect_view()
                    }}
                </div>
                <div
                    class="lib-elided-chain"
                    on:mouseenter=move |_| intent.enter()
                    on:mouseleave=move |_| intent.leave()
                >
                    {move || {
                        let last_id = folded
                            .get_value()
                            .last()
                            .map(|crumb| crumb.id.clone())
                            .unwrap_or_default();
                        rows.get()
                            .into_iter()
                            .map(|row| {
                                let last_of_chain = last_id.clone();
                                view! {
                                    <div class="lib-elided-row">
                                        {row
                                            .into_iter()
                                            .map(|crumb| {
                                                let trails = crumb.id != last_of_chain;
                                                view! {
                                                    <ElidedCrumb
                                                        state=state
                                                        ctrl=ctrl
                                                        crumb=crumb
                                                        trails=trails
                                                        intent=intent
                                                    />
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                }
                            })
                            .collect_view()
                    }}
                </div>
            </MenuPopover>
        </div>
    }
}

/// Drawn the way the bar draws it — name, chevron, name — so a reader who
/// understands the breadcrumb already understands the panel, three levels
/// wide where a list of rows would be three levels tall.
#[component]
fn ElidedCrumb(
    state: crate::context::LibraryContext,
    ctrl: DragController,
    crumb: Crumb,
    trails: bool,
    intent: HoverIntent,
) -> impl IntoView {
    let id = crumb.id.clone();
    let label = crumb.name.clone();
    let tooltip = crumb.name;
    let dom_id = register_crumb(&ctrl, &id);
    let hot_id = id.clone();
    let click_id = id;

    view! {
        <span class="lib-elided-crumb">
            <button
                id=dom_id
                type="button"
                title=tooltip
                on:click=move |_| state.library.shelf.set(click_id.clone())
                on:mouseenter=move |_| intent.enter()
                on:mouseleave=move |_| intent.leave()
                class=move || {
                    let base = "flex min-w-0 max-w-32 items-center rounded-md px-1.5 py-0.5 \
                                text-sm text-muted transition-colors hover:bg-line \
                                hover:text-ink focus:outline-none focus-visible:ring-2 \
                                focus-visible:ring-accent";
                    if ctrl.over_shelf(&hot_id) {
                        format!("{base} crumb-drop")
                    } else {
                        base.to_string()
                    }
                }
            >
                <span class="truncate">{label}</span>
            </button>
            {trails.then(|| {
                view! { <Icon name=IconName::Next size=13 class="shrink-0 text-muted" /> }
            })}
        </span>
    }
}
