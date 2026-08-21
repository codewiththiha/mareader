//! Zoom controls. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): zoom in/out (stepping through the presets in math::ZOOM_STEPS),
//! fit width / fit page, and a percent readout + popover replacing the old preset
//! Select. Any manual zoom clears the fit mode. The readout reads `viewer.scale`
//! directly, so a non-preset fit value like 137% shows correctly.
//!
//! The popover renders through the shared window-aware `Popover`, which owns
//! outside-click/Escape dismissal, viewport clamping and the titlebar hold.

use std::sync::Arc;

use leptos::html;
use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_core::math::{fit_scale, is_space_constrained, nearest_zoom, FitMode, ZOOM_STEPS};
use crate::components::chrome::adaptive_group::{OverflowRow, ToolbarEntry};
use crate::components::chrome::popover::Popover;
use crate::core::state::AppState;
use pdf_viewer::effects::zoom::request_zoom;

/// Apply a manual zoom level: exit fit mode, then hand the target to the zoom
/// coordinator.
///
/// It must NOT write `scale`/`render_scale` itself. Doing that was the original
/// bug: the scale changed instantly while the wrappers' `top:` offsets and the
/// spacer height only caught up as each render resolved, so the scroll offset
/// ended up pointing at a different page. `request_zoom` animates the layout
/// and re-anchors the scroll in the same frames, then renders once.
pub(crate) fn apply_zoom(state: AppState, scale: f64) {
    state.viewer.fit.set(FitMode::None);
    request_zoom(state.viewer_state(), scale, true);
}

/// The zoom a `+`/`-` step should be measured from: the target of an in-flight
/// gesture if there is one, else what is on screen. See `shortcuts::zoom_by`
/// for why neither `scale` nor `display_scale` alone is correct — without this,
/// clicking `+` twice quickly moves only one preset.
fn step_base(state: AppState) -> f64 {
    state
        .viewer
        .zoom_request
        .get_untracked()
        .filter(|_| state.viewer.zoom_animating.get_untracked())
        .map(|(target, _, _)| target)
        .unwrap_or_else(|| state.viewer.display_scale.get_untracked())
}

/// Toolbar entries for the collision-aware reader bar (fit, zoom, readout).
pub fn zoom_entries(state: AppState) -> Vec<ToolbarEntry> {
    let zoom_step_inline: Arc<dyn Fn() -> AnyView + Send + Sync> = Arc::new(move || {
        view! {
            <div class="flex items-center gap-1">
                <Tooltip text="Zoom out (-)".to_string()>
                    <Button
                        on_click=move |_| apply_zoom(state, nearest_zoom(step_base(state), -1))
                        kind=ButtonKind::Ghost
                        icon=IconName::ZoomOut
                        title="Zoom out (-)".to_string()
                    />
                </Tooltip>
                <Tooltip text="Zoom in (+)".to_string()>
                    <Button
                        on_click=move |_| apply_zoom(state, nearest_zoom(step_base(state), 1))
                        kind=ButtonKind::Ghost
                        icon=IconName::ZoomIn
                        title="Zoom in (+)".to_string()
                    />
                </Tooltip>
            </div>
        }
        .into_any()
    });

    vec![
        // Zoom out + zoom in are one entry so they collapse together and stay
        // on one horizontal row in both the bar and the overflow menu.
        ToolbarEntry {
            id: "zoom-step",
            priority: 80,
            keep_mounted: false,
            inline: zoom_step_inline.clone(),
            sizer: zoom_step_inline,
            collapsed: Arc::new(move |_done| {
                view! {
                    <div class="flex w-full items-center gap-1 px-1 py-0.5">
                        <button
                            type="button"
                            title="Zoom out"
                            on:click=move |_| {
                                apply_zoom(state, nearest_zoom(step_base(state), -1));
                            }
                            class="inline-flex h-9 flex-1 items-center justify-center gap-1 rounded-md text-sm text-ink hover:bg-line"
                        >
                            <Icon name=IconName::ZoomOut size=14 />
                            <span>Out</span>
                        </button>
                        <span class="h-4 w-px shrink-0 bg-line"></span>
                        <button
                            type="button"
                            title="Zoom in"
                            on:click=move |_| {
                                apply_zoom(state, nearest_zoom(step_base(state), 1));
                            }
                            class="inline-flex h-9 flex-1 items-center justify-center gap-1 rounded-md text-sm text-ink hover:bg-line"
                        >
                            <Icon name=IconName::ZoomIn size=14 />
                            <span>In</span>
                        </button>
                    </div>
                }
                .into_any()
            }),
        },
        // fit-width and fit-page entries live in `fit_entry` (reader_view.rs)
        zoom_readout_entry(state),
    ]
}

fn zoom_readout_entry(state: AppState) -> ToolbarEntry {
    ToolbarEntry {
        id: "zoom-readout",
        priority: u32::MAX,
        keep_mounted: true,
        inline: Arc::new(move || view! { <ZoomReadout state=state /> }.into_any()),
        sizer: Arc::new(move || {
            view! {
                <div class="inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium border-line bg-surface text-ink">
                    <span>"100%"</span>
                    <svg
                        class="text-muted"
                        width="12"
                        height="12"
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                    >
                        <path d="m6 9 6 6 6-6"/>
                    </svg>
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
    let percent = move || format!("{}%", (state.viewer.scale.get() * 100.0).round() as u32);
    let zoom_title = move || {
        let shown = state.viewer.scale.get();
        let desired = state.viewer.desired_scale.get();
        let (cw, ch) = state.viewer.container_size.get();
        let held_back = state
            .doc
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
    let trigger_class = move || {
        let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";
        if open.get() {
            format!("{base} border-accent text-accent")
        } else {
            base.to_string()
        }
    };
    view! {
        <div node_ref=root_ref class="relative inline-flex">
            <button
                type="button"
                data-zoom-readout="true"
                title=zoom_title
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                <span>{percent}</span>
                <svg
                    class="text-muted"
                    width="12"
                    height="12"
                    viewBox="0 0 24 24"
                    fill="none"
                    stroke="currentColor"
                    stroke-width="2"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                >
                    <path d="m6 9 6 6 6-6"/>
                </svg>
            </button>
            <Popover open=open anchor=root_ref width=176 class="p-1".to_string()>
                <button
                    type="button"
                    on:click=move |_| {
                        state.viewer.fit.set(FitMode::Width);
                        open.set(false);
                    }
                    class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
                >
                    <span class="inline-flex w-4 shrink-0 justify-center text-accent">
                        {move || (state.viewer.fit.get() == FitMode::Width).then(|| view! { <Icon name=IconName::Check size=14/> })}
                    </span>
                    <span>Fit width</span>
                </button>
                <button
                    type="button"
                    on:click=move |_| {
                        state.viewer.fit.set(FitMode::Page);
                        open.set(false);
                    }
                    class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
                >
                    <span class="inline-flex w-4 shrink-0 justify-center text-accent">
                        {move || (state.viewer.fit.get() == FitMode::Page).then(|| view! { <Icon name=IconName::Check size=14/> })}
                    </span>
                    <span>Fit page</span>
                </button>
                <Separator vertical=false />
                <For
                    each=move || ZOOM_STEPS.iter().copied()
                    key=|z| z.to_bits()
                    children=move |z| {
                        let row_class = move || {
                            let base = "flex w-full items-center justify-between gap-2 rounded-md px-2 py-1.5 text-sm";
                            if (state.viewer.scale.get() - z).abs() < 1e-9 {
                                format!("{base} bg-accent-soft text-accent")
                            } else {
                                format!("{base} text-ink hover:bg-line")
                            }
                        };
                        view! {
                            <button
                                type="button"
                                on:click=move |_| {
                                    apply_zoom(state, z);
                                    open.set(false);
                                }
                                class=row_class
                            >
                                <span>{format!("{}%", (z * 100.0).round() as u32)}</span>
                                {move || ((state.viewer.scale.get() - z).abs() < 1e-9).then(|| view! { <Icon name=IconName::Check size=14/> })}
                            </button>
                        }
                    }
                />
            </Popover>
        </div>
    }
}
