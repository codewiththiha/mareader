//! Zoom controls: zoom in/out (stepping through the presets in math::ZOOM_STEPS),
//! fit width / fit page, and a percent readout + popover replacing the old preset
//! Select. Any manual zoom clears the fit mode. The readout reads `viewer.zoom.scale`
//! directly, so a non-preset fit value like 137% shows correctly.
//!
//! The popover renders through the shared window-aware `Popover`, which owns
//! outside-click/Escape dismissal, viewport clamping and the titlebar hold.

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use crate::components::primitives::button::{Button, ButtonVariant};
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::menu_item::MenuItem;
use crate::components::primitives::separator::Separator;
use crate::components::primitives::tooltip::Tooltip;
use pdf_core::math::{fit_scale, is_space_constrained, nearest_zoom, FitMode, ZOOM_STEPS};
use crate::components::app_shell::adaptive_toolbar::ToolbarItem;
use crate::components::app_shell::OverflowRow;
use crate::components::app_shell::toolbar_popover::MenuPopover;
use crate::state::AppState;
use crate::effects::reader::zoom::request_zoom;

/// Apply a manual zoom level: exit fit mode, then hand the target to the zoom
/// coordinator.
///
/// It must NOT write `scale`/`render_scale` itself. Doing that was the original
/// bug: the scale changed instantly while the wrappers' `top:` offsets and the
/// spacer height only caught up as each render resolved, so the scroll offset
/// ended up pointing at a different page. `request_zoom` animates the layout
/// and re-anchors the scroll in the same frames, then renders once.
pub(crate) fn apply_zoom(state: AppState, scale: f64) {
    state.reader.viewer.fit.set(FitMode::None);
    request_zoom(state.reader, scale, true);
}

/// The zoom a `+`/`-` step should be measured from: the target of an in-flight
/// gesture if there is one, else what is on screen. See `shortcuts::zoom_by`
/// for why neither `scale` nor `display_scale` alone is correct — without this,
/// clicking `+` twice quickly moves only one preset.
fn step_base(state: AppState) -> f64 {
    state
        .reader
        .viewer
        .zoom
        .request
        .get_untracked()
        .filter(|_| state.reader.viewer.zoom_animating.get_untracked())
        .map(|(target, _, _)| target)
        .unwrap_or_else(|| state.reader.viewer.zoom.display.get_untracked())
}

/// Toolbar entries for the collision-aware reader bar (fit, zoom, readout).
pub fn zoom_entries(state: AppState) -> Vec<ToolbarItem> {
    vec![
        // Zoom out + zoom in are one entry so they collapse together and stay
        // on one horizontal row in both the bar and the overflow menu.
        ToolbarItem::pair(
            "zoom-step",
            80,
            move || {
                view! {
                    <div class="flex items-center gap-1">
                        <Tooltip text="Zoom out (-)">
                            <Button
                                on_click=move |_| apply_zoom(state, nearest_zoom(step_base(state), -1))
                                variant=ButtonVariant::Ghost
                                title="Zoom out (-)"
                            >
                                <Icon name=IconName::ZoomOut size=18 />
                            </Button>
                        </Tooltip>
                        <Tooltip text="Zoom in (+)">
                            <Button
                                on_click=move |_| apply_zoom(state, nearest_zoom(step_base(state), 1))
                                variant=ButtonVariant::Ghost
                                title="Zoom in (+)"
                            >
                                <Icon name=IconName::ZoomIn size=18 />
                            </Button>
                        </Tooltip>
                    </div>
                }
                .into_any()
            },
            move |_done| {
                view! {
                    <div class="flex w-full flex-col">
                        <MenuItem
                            icon=IconName::ZoomOut
                            label="Out"
                            on_click=move || apply_zoom(state, nearest_zoom(step_base(state), -1))
                        />
                        <MenuItem
                            icon=IconName::ZoomIn
                            label="In"
                            on_click=move || apply_zoom(state, nearest_zoom(step_base(state), 1))
                        />
                    </div>
                }
                .into_any()
            },
        ),
        // fit-width and fit-page entries live in `fit_entry` (features/reader/page.rs)
        zoom_readout_entry(state),
    ]
}

fn zoom_readout_entry(state: AppState) -> ToolbarItem {
    ToolbarItem {
        id: "zoom-readout",
        priority: u32::MAX,
        keep_mounted: true,
        inline: Arc::new(move || view! { <ZoomReadout state=state /> }.into_any()),
        sizer: Arc::new(move || {
            view! {
                <div class="inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium border-line bg-surface text-ink">
                    <span>"100%"</span>
                    <Icon name=IconName::ChevronDown size=12 class="text-muted" />
                </div>
            }
            .into_any()
        }),
        collapsed: Arc::new(move |done| {
            view! {
                <OverflowRow icon=IconName::ZoomIn label="Zoom" done=done on_click=move || {} />
            }
            .into_any()
        }),
    }
}

/// Percent readout + preset popover, extracted so it can be a single entry.
#[component]
fn ZoomReadout(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();
    let percent = move || format!("{}%", (state.reader.viewer.zoom.scale.get() * 100.0).round() as u32);
    let zoom_title = move || {
        let shown = state.reader.viewer.zoom.scale.get();
        let desired = state.reader.viewer.zoom.desired.get();
        let (cw, ch) = state.reader.viewer.container_size.get();
        let held_back = state
            .reader
            .document
            .page1_size
            .get()
            .map(|p| {
                let fit_w = fit_scale(FitMode::Width, cw, ch, p.width, p.height, 48.0, shown);
                is_space_constrained(desired, fit_w)
            })
            .unwrap_or(false);
        if held_back {
            format!(
                "Zoom — fitted to the window; returns to {}% when there is room",
                (desired * 100.0).round() as u32
            )
        } else {
            "Zoom".to_string()
        }
    };
    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <Button
                on_click=move |_| open.set(!open.get())
                variant=ButtonVariant::Toolbar
                active=Signal::derive(move || open.get())
                title=Signal::derive(zoom_title)
            >
                <span>{percent}</span>
                <Icon name=IconName::ChevronDown size=12 class="text-muted" />
            </Button>
            <MenuPopover open=open anchor=root_ref width=176 coordinate_space="toolbar-row" class="p-1".to_string()>
                <MenuItem
                    label="Fit width"
                    selected=Signal::derive(move || state.reader.viewer.fit.get() == FitMode::Width)
                    on_click=move || {
                        state.reader.viewer.fit.set(FitMode::Width);
                        open.set(false);
                    }
                >
                    <span class="ml-auto inline-flex w-4 shrink-0 justify-center text-accent">
                        {move || (state.reader.viewer.fit.get() == FitMode::Width).then(|| view! { <Icon name=IconName::Check size=14/> })}
                    </span>
                </MenuItem>
                <MenuItem
                    label="Fit page"
                    selected=Signal::derive(move || state.reader.viewer.fit.get() == FitMode::Page)
                    on_click=move || {
                        state.reader.viewer.fit.set(FitMode::Page);
                        open.set(false);
                    }
                >
                    <span class="ml-auto inline-flex w-4 shrink-0 justify-center text-accent">
                        {move || (state.reader.viewer.fit.get() == FitMode::Page).then(|| view! { <Icon name=IconName::Check size=14/> })}
                    </span>
                </MenuItem>
                <Separator vertical=false />
                <For
                    each=move || ZOOM_STEPS.iter().copied()
                    key=|z| z.to_bits()
                    children=move |z| {
                        view! {
                            <MenuItem
                                label=format!("{}%", (z * 100.0).round() as u32)
                                selected=Signal::derive(move || (state.reader.viewer.zoom.scale.get() - z).abs() < 1e-9)
                                on_click=move || {
                                    apply_zoom(state, z);
                                    open.set(false);
                                }
                            >
                                <span class="ml-auto inline-flex w-4 shrink-0 justify-center text-accent">
                                    {move || ((state.reader.viewer.zoom.scale.get() - z).abs() < 1e-9).then(|| view! { <Icon name=IconName::Check size=14/> })}
                                </span>
                            </MenuItem>
                        }
                    }
                />
            </MenuPopover>
        </div>
    }
}
