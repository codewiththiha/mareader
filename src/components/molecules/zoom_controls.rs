//! Zoom controls. OWNED BY branch B (viewer/chrome).
//! Zoom in/out (stepping through the presets in math::ZOOM_STEPS), fit width,
//! fit page, and a preset-% Select. Any manual zoom clears the fit mode.

use leptos::prelude::*;

use crate::components::atoms::button::{Button, ButtonKind};
use crate::components::atoms::icon::IconName;
use crate::components::atoms::select::Select;
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
    let zoom_out_state = state;
    let zoom_in_state = state;
    let fit_width_state = state;
    let fit_page_state = state;
    let on_change_state = state;

    let options: Vec<(f64, String)> = ZOOM_STEPS
        .iter()
        .map(|&z| (z, format!("{}%", (z * 100.0).round() as u32)))
        .collect();
    let value = state.viewer.scale.read_only();

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
            <Select
                options=options
                value=value
                on_change=move |z: f64| apply_zoom(on_change_state, z)
                title="Zoom".to_string()
            />
        </div>
    }
}
