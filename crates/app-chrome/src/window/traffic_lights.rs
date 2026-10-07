//! The native macOS traffic lights: two hosts, one pair, and a hide
//! that always lands.

use std::time::Duration;

use leptos::prelude::*;

use crate::hooks::frame_active::use_frame_active;
use crate::hooks::use_resize_observer::observe_elements;
use crate::hooks::verified_switch::use_verified_switch;
use crate::titlebar::TITLE_BAR_H;
use crate::titlebar::root::TitleBarCtx;
use crate::window::api::set_traffic_lights;

#[component]
pub fn TrafficLights(
    /// The rail or its close motion owns the lights' corner right now.
    rail_hosted: Signal<bool>,
    /// The bar may host the lights in this layout.
    bar_hosted: Signal<bool>,
) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let active = use_frame_active();
    let hide_grace = StoredValue::new_local(None::<TimeoutHandle>);
    // Live header height for Tahoe-proof centering. Observed on
    // `#toolbar-row`; `on_cleanup` in `observe_elements` disconnects it.
    let header_height: RwSignal<f64> = RwSignal::new(TITLE_BAR_H);

    // Keep `header_height` in sync with the real bar height.
    Effect::new(move |_| {
        let Some(row) = ctx.and_then(|c| c.row_ref.get()) else {
            return;
        };
        let hh = header_height;
        observe_elements(vec![row.into()], move |entries| {
            if let Some(entry) = entries.first() {
                let h = entry.content_rect().height();
                if h > 0.0 {
                    hh.set(h);
                }
            }
        });
    });

    // The live truth about the lights, read untracked.
    let truth = move || {
        // A hidden frame abstains: its correction would fight the active one.
        if !active.try_get_untracked().unwrap_or(false) {
            return None;
        }
        Some(
            rail_hosted.get_untracked()
                || (bar_hosted.get_untracked() && ctx.is_some_and(|c| c.visible.get_untracked())),
        )
    };
    // Send-and-verify lives in the shared hook.
    let lights = use_verified_switch(truth, |want, height: f64| set_traffic_lights(want, height));

    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let clear_grace = || {
            if let Some(h) = hide_grace.get_value() {
                h.clear();
                hide_grace.set_value(None);
            }
        };
        if !active.get() {
            // Not on screen: drop the pending hide and forget what was sent.
            clear_grace();
            lights.forget();
            return;
        }
        let on = rail_hosted.get() || (bar_hosted.get() && ctx.visible.get());
        // Re-read header_height so every transition carries the centered `y`.
        let h = header_height.get();
        if on {
            // Never send the end-of-slide false if hover re-enters on the
            // following frame.
            clear_grace();
            if lights.last_sent() == Some(true) {
                return;
            }
            lights.send(true, h);
        } else {
            // An already-hidden or pending hide does not schedule another
            // command.
            if lights.last_sent() == Some(false) || hide_grace.get_value().is_some() {
                return;
            }
            // The grace exists for the docked handoff.
            if !bar_hosted.get_untracked() {
                lights.send(false, h);
                return;
            }
            let lights = lights.clone();
            let handle = set_timeout_with_handle(
                move || {
                    hide_grace.set_value(None);
                    // The frame may have been hidden during the grace.
                    if active.get_untracked() {
                        lights.send(false, h);
                    }
                },
                Duration::from_millis(120),
            )
            .ok();
            hide_grace.set_value(handle);
        }
    });

    on_cleanup(move || {
        if let Some(h) = hide_grace.get_value() {
            h.clear();
        }
    });
}
