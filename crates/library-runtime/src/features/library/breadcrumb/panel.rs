//! The ellipsis and its panel: the folded levels, the hover intent, the rows.

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

/// Not in the drag's registry: a measurement target is not a drop.
const ELIDED_MAX_DOM_ID: &str = "crumb-elided-max";

/// The floor keeps a sliver of a window from producing no budget at all.
fn elided_budget_px() -> f64 {
    web_sys::window()
        .and_then(|w| w.inner_width().ok())
        .and_then(|v| v.as_f64())
        .map(|window| (window - 24.0).max(160.0))
        .unwrap_or(0.0)
}

/// Not a `crumb-` id: the registry would mistake it for a crumb.
const ELLIPSIS_DOM_ID: &str = "crumb-elided";

const MENU_CLOSE_GRACE_MS: u64 = 220;

/// A beat behind the leave, owned by an effect on `over`: one timer.
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
                    // A grace timer can outlive the panel: a disposed
                    // signal means the menu went too.
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

/// A button, not an arrow: a keyboard reaches the panel too.
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
    // Children are an `Fn`; an owned list would be an `FnOnce`.
    let folded: StoredValue<Vec<Crumb>, LocalStorage> = StoredValue::new_local(elided);
    let rows: RwSignal<Vec<Vec<Crumb>>> = RwSignal::new(vec![folded.get_value()]);

    // The width is measured, the chain packed: rows are surfaces of their own.
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
    // Measured on mount and on resize: the width moves with chain or window.
    Effect::new(move |_| {
        if !intent.open.get() {
            return;
        }
        request_animation_frame(measure);
        let handle = window_event_listener(leptos::ev::resize, move |_| measure());
        on_cleanup(move || handle.remove());
    });

    // A drag raises no `mouseenter`, so the live session opens the panel.
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
            // Each crumb registers a drop target that must
            // leave the registry on unmount.
            <MenuPopover
                open=intent.open
                anchor=anchor
                width=Signal::derive(move || panel_width.get() as u32)
                coordinate_space="toolbar-row"
                class="lib-elided-panel max-h-80 overflow-y-auto".to_string()
            >
                // The ruler wears the crumbs' metrics; one width per crumb.
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

/// Drawn as the bar draws it, so the panel needs no second reading.
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
