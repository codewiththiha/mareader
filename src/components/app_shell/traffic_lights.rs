//! Native traffic lights: on while pinned/hovered, or while the sidebar is
//! painted. A short hide grace absorbs the end-of-close hover-band handoff:
//! the rail releases chrome, then the expanded band can immediately put the
//! stationary pointer back over the bar.

use std::time::Duration;

use leptos::prelude::*;

use crate::components::app_shell::title_bar::TitleBarCtx;

#[component]
pub fn TrafficLights(present: Signal<bool>) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let last_sent = StoredValue::new_local(None::<bool>);
    let hide_grace = StoredValue::new_local(None::<TimeoutHandle>);

    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = ctx.visible.get() || present.get();
        if on {
            // Never send the end-of-slide false if hover re-enters on the
            // following frame.
            if let Some(h) = hide_grace.get_value() {
                h.clear();
                hide_grace.set_value(None);
            }
            if last_sent.get_value() == Some(true) {
                return;
            }
            last_sent.set_value(Some(true));
            wasm_bindgen_futures::spawn_local(async move {
                pdf_engine::api::set_traffic_lights(true).await;
            });
        } else {
            // Initial false also receives the grace so native defaults are
            // eventually hidden; an already-hidden or pending hide does not
            // schedule another command.
            if last_sent.get_value() == Some(false) || hide_grace.get_value().is_some() {
                return;
            }
            let handle = set_timeout_with_handle(
                move || {
                    hide_grace.set_value(None);
                    last_sent.set_value(Some(false));
                    wasm_bindgen_futures::spawn_local(async move {
                        pdf_engine::api::set_traffic_lights(false).await;
                    });
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
