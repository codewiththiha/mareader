//! Zoom controls. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): zoom in/out (stepping through the presets in math::ZOOM_STEPS),
//! fit width / fit page, and a percent readout + popover replacing the old preset
//! Select. Any manual zoom clears the fit mode. The readout reads `viewer.scale`
//! directly, so a non-preset fit value like 137% shows correctly.

use leptos::html;
use leptos::prelude::*;

use pdf_viewer::components::atoms::button::{Button, ButtonKind};
use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_viewer::components::atoms::separator::Separator;
use pdf_viewer::components::atoms::tooltip::Tooltip;
use pdf_core::math::{fit_scale, is_space_constrained, nearest_zoom, FitMode, ZOOM_STEPS};
use crate::core::state::AppState;
use pdf_viewer::effects::fit::request_zoom;

/// Apply a manual zoom level: exit fit mode, then hand the target to the zoom
/// coordinator.
///
/// It must NOT write `scale`/`render_scale` itself. Doing that was the original
/// bug: the scale changed instantly while the wrappers' `top:` offsets and the
/// spacer height only caught up as each render resolved, so the scroll offset
/// ended up pointing at a different page. `request_zoom` animates the layout
/// and re-anchors the scroll in the same frames, then renders once.
fn apply_zoom(state: AppState, scale: f64) {
    state.viewer.fit.set(FitMode::None);
    request_zoom(
        pdf_viewer::state::ViewerState::new(state.doc, state.viewer, state.search, state.sidebar),
        scale,
        true,
    );
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

#[component]
pub fn ZoomControls(state: AppState) -> impl IntoView {
    
    let open = RwSignal::new(false);
    let root_ref: NodeRef<html::Div> = NodeRef::new();

    // Outside-click dismiss (U7): while the popover is open, any pointerdown
    // landing outside the control closes it. Re-registered per open-flip,
    // removed on cleanup (same lifecycle as the floating-search overlay).
    Effect::new(move |_| {
        if open.get() {
            let handle = window_event_listener(
                leptos::ev::pointerdown,
                move |ev: leptos::ev::PointerEvent| {
                    let target: web_sys::Node = event_target(&ev);
                    let contains = root_ref
                        .get()
                        .as_ref()
                        .is_some_and(|c| c.contains(Some(&target)));
                    if !contains {
                        open.set(false);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        }
    });

    // Escape dismiss (U7): same window-listener lifecycle.
    Effect::new(move |_| {
        if open.get() {
            let handle = window_event_listener(
                leptos::ev::keydown,
                move |ev: leptos::ev::KeyboardEvent| {
                    if ev.key() == "Escape" {
                        open.set(false);
                    }
                },
            );
            on_cleanup(move || handle.remove());
        }
    });

    let zoom_out_state = state;
    let zoom_in_state = state;
    let fit_width_state = state;
    let fit_page_state = state;

    let percent = move || format!("{}%", (state.viewer.scale.get() * 100.0).round() as u32);

    // When the window (or the sidebar) leaves too little room, the page is held
    // BELOW the zoom the reader picked so it fits instead of being cropped.
    // The readout then shows a percentage nobody chose, which looks like the
    // app forgot the setting — so the tooltip says what is going on and what it
    // will return to. Purely explanatory: the number itself stays honest about
    // what is on screen.
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
        <div node_ref=root_ref class="flex items-center gap-1">
            <Tooltip text="Zoom out (-)".to_string()>
                <Button
                    on_click=move |_| {
                        let cur = step_base(zoom_out_state);
                        apply_zoom(zoom_out_state, nearest_zoom(cur, -1));
                    }
                    kind=ButtonKind::Ghost
                    icon=IconName::ZoomOut
                    title="Zoom out (-)".to_string()
                />
            </Tooltip>
            <Tooltip text="Zoom in (+)".to_string()>
                <Button
                    on_click=move |_| {
                        let cur = step_base(zoom_in_state);
                        apply_zoom(zoom_in_state, nearest_zoom(cur, 1));
                    }
                    kind=ButtonKind::Ghost
                    icon=IconName::ZoomIn
                    title="Zoom in (+)".to_string()
                />
            </Tooltip>
            <Tooltip text="Fit width (Cmd/Ctrl+0)".to_string()>
                <Button
                    on_click=move |_| fit_width_state.viewer.fit.set(FitMode::Width)
                    kind=ButtonKind::Ghost
                    icon=IconName::FitWidth
                    title="Fit width (Cmd/Ctrl+0)".to_string()
                />
            </Tooltip>
            <Tooltip text="Fit page".to_string()>
                <Button
                    on_click=move |_| fit_page_state.viewer.fit.set(FitMode::Page)
                    kind=ButtonKind::Ghost
                    icon=IconName::FitPage
                    title="Fit page".to_string()
                />
            </Tooltip>

            // Percent readout + popover (replaces the old preset Select).
            <div class="relative inline-flex">
                <button
                    type="button"
                    // Stable hook for tests and tooling. `title` is USER-FACING
                    // copy that now changes to explain a space-constrained
                    // zoom, so it is not an identity — anything selecting this
                    // control must use this attribute instead.
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
                <Show when=move || open.get()>
                    <div class="menu-popover absolute right-0 top-full z-50 mt-1 w-44 rounded-lg border border-line bg-surface p-1 shadow-lg">
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
                    </div>
                </Show>
            </div>
        </div>
    }
}
