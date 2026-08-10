//! Zoom controls. OWNED BY branch B (viewer/chrome).
//! Redesigned (U2): zoom in/out (stepping through the presets in math::ZOOM_STEPS),
//! fit width / fit page, and a percent readout + popover replacing the old preset
//! Select. Any manual zoom clears the fit mode. The readout reads `viewer.scale`
//! directly, so a non-preset fit value like 137% shows correctly.

use leptos::prelude::*;

use crate::components::atoms::button::{Button, ButtonKind};
use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::separator::Separator;
use crate::components::atoms::tooltip::Tooltip;
use crate::core::math::{clamp_scale, nearest_zoom, FitMode, ZOOM_STEPS};
use crate::core::state::AppState;

/// Apply a manual zoom level: exit fit mode, then set scale + render_scale.
fn apply_zoom(state: AppState, scale: f64) {
    let z = clamp_scale(scale);
    state.viewer.fit.set(FitMode::None);
    state.viewer.scale.set(z);
    state.viewer.render_scale.set(z);
}

#[component]
pub fn ZoomControls(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);

    let zoom_out_state = state;
    let zoom_in_state = state;
    let fit_width_state = state;
    let fit_page_state = state;

    let percent = move || format!("{}%", (state.viewer.scale.get() * 100.0).round() as u32);

    let trigger_class = move || {
        let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";
        if open.get() {
            format!("{base} border-accent text-accent")
        } else {
            base.to_string()
        }
    };

    view! {
        <div class="flex items-center gap-1">
            <Tooltip text="Zoom out (-)".to_string()>
                <Button
                    on_click=move |_| {
                        let cur = zoom_out_state.viewer.scale.get();
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
                        let cur = zoom_in_state.viewer.scale.get();
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
                    title="Zoom"
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
