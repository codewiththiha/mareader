//! The native macOS traffic lights: on while the bar is pinned/hovered, or
//! while the rail is painted. A short hide grace absorbs the end-of-close
//! hover-band handoff: the rail releases chrome, then the expanded band can
//! immediately put the stationary pointer back over the bar.
//!
//! TWO HOSTS, ONE PAIR OF LIGHTS. The lights are native and pinned to the
//! window's top-left, so whichever surface owns that corner is the one they
//! sit on: the title bar's 88px gutter when the rail is down, the rail's
//! own header gutter when it is up — docked or floating, the header
//! reserves the same 88px either way. The app computes those two hosting
//! facts (its shell controller owns the rail's open/close machine) and
//! passes them down as `rail_hosted` / `bar_hosted`: this component stays
//! chrome, not app — it does not know what a sidebar is.
//!
//! The grace at the hide is the BAR's, so it only applies where the bar can
//! take the lights back (`bar_hosted`): in an overlay layout there is no
//! gutter to hand off to and nothing hover-gated coming back, so the hide
//! lands in the same frame the floating rail finishes fading — which is the
//! whole point of the fade's timing.
//!
//! THE HIDE ALWAYS LANDS. The lights follow the bar's hover-reveal through
//! [`TitleBarCtx::visible`], and the bar's hide is re-checked at both ends
//! of its hold (see `app_chrome::titlebar::root`) — so the decision here is
//! sound, but the command is async IPC while the decision is synchronous: a
//! decision that changes mid-flight could otherwise let a stale command land
//! last, leaving the native lights up with nothing left to re-run the
//! effect. Every send therefore re-checks the live truth once its promise
//! resolves and answers a mismatch — the lights' version of the bar's own
//! recheck, and why an unfocus always ends with the lights gone.
//!
//! Dynamic `y` (Tahoe-proof centering) — mirrors `readest` `traffic_light.rs`
//! `compute_traffic_light_y + OnceLock + ResizeObserver`. The bar is `h-12`
//! (48px, `TOOLBAR_H`) but `y` is NOT `tauri.conf.json:trafficLightPosition`
//! (that's only the pre-mount fallback). The live header height is measured
//! from `#toolbar-row` via `ResizeObserver`; every `visible=true` invoke
//! carries it as `headerHeight`, and the Rust command owns
//! `y = ((h - btn_h)/2 + natural_origin_y).max(0)` with a cached
//! `natural_origin_y` (~5pt Sonoma, ~7pt Tahoe) so no per-OS branch.

use std::time::Duration;

use leptos::prelude::*;

use crate::hooks::dom::{by_id, TOOLBAR_ROW_ID};
use crate::hooks::use_async_truth::use_async_truth;
use crate::hooks::use_resize_observer::observe_elements;
use crate::titlebar::root::TitleBarCtx;
use crate::window::api::set_traffic_lights;

const DEFAULT_HEADER_HEIGHT: f64 = 48.0; // h-12, mirrors Rust fallback

#[component]
pub fn TrafficLights(
    /// The rail (or its close motion) owns the lights' corner right now:
    /// their host is the rail's header gutter, independent of the bar's
    /// visibility.
    rail_hosted: Signal<bool>,
    /// The bar may host the lights in this layout (it owes them a gutter).
    /// While true the lights follow the bar's visibility, and a hide waits
    /// out the handoff grace; while false a hide lands immediately, because
    /// there is no bar corner for the lights to hand back to.
    bar_hosted: Signal<bool>,
) -> impl IntoView {
    let ctx = use_context::<TitleBarCtx>();
    let hide_grace = StoredValue::new_local(None::<TimeoutHandle>);
    // Live header height for Tahoe-proof centering. Observed on
    // `#toolbar-row`; `on_cleanup` in `observe_content_size` disconnects it.
    let header_height: RwSignal<f64> = RwSignal::new(DEFAULT_HEADER_HEIGHT);

    // Keep `header_height` in sync with the real bar height. This is what
    // replaces the static `tauri.conf.json {y:25}` with a live value. The
    // observer fires once on `observe()` with the current size, so the
    // first `visible=true` invoke already carries the centered `y`.
    Effect::new(move |_| {
        let Some(el) = by_id(TOOLBAR_ROW_ID) else {
            return;
        };
        let hh = header_height;
        observe_elements(vec![el], move |entries| {
            if let Some(entry) = entries.first() {
                let h = entry.content_rect().height();
                if h > 0.0 {
                    hh.set(h);
                }
            }
        });
    });

    // The live truth about whether the lights should be on, read WITHOUT
    // subscribing — the verification pass probes it after the command has
    // landed, where a tracked read would create a dependency on an owner
    // that no longer runs it.
    let truth = move || {
        rail_hosted.get_untracked()
            || (bar_hosted.get_untracked() && ctx.is_some_and(|c| c.visible.get_untracked()))
    };
    // Send-and-verify lives in the shared hook: the decision is recorded,
    // the IPC awaited, and a truth that moved mid-flight answered with one
    // more command — so a stale send can never settle the native side last.
    let lights = use_async_truth(truth, |want, height: f64| set_traffic_lights(want, height));

    Effect::new(move |_| {
        let Some(ctx) = ctx else {
            return;
        };
        let on = rail_hosted.get() || (bar_hosted.get() && ctx.visible.get());
        // Re-read header_height so every transition (hover in/out,
        // sidebar slide, resize) carries the current centered `y` — Rust
        // re-applies `ThemeChanged` without IPC, but JS must send the
        // height on each visibility toggle.
        let h = header_height.get();
        if on {
            // Never send the end-of-slide false if hover re-enters on the
            // following frame.
            if let Some(h) = hide_grace.get_value() {
                h.clear();
                hide_grace.set_value(None);
            }
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
            // The grace exists for the docked handoff (rail releases chrome,
            // the re-widened band can re-light them under a stationary
            // pointer). Where the bar never hosts the lights there is
            // nothing to wait for — and waiting would leave three native
            // buttons floating over the rail that has just finished fading
            // out from under them.
            if !bar_hosted.get_untracked() {
                lights.send(false, h);
                return;
            }
            let lights = lights.clone();
            let handle = set_timeout_with_handle(
                move || {
                    hide_grace.set_value(None);
                    lights.send(false, h);
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
