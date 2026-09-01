//! The effect and listener installation for the auto-center machinery: the
//! reveal-active gesture, the open-snap + page-follow effect, the
//! self-re-arming glide, and the panel-lifetime cleanup.
//!
//! This file only wires the pure timing rules from [`super::math`] to the
//! panel's signals, timers and virtualizer — it makes no decisions of its
//! own.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::components::shell::sidebar::panels::thumbnails::geometry::{GRACE_MS, THUMB_SCALE};
use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

use super::math::{center_target, glide_delay, glide_verdict, GlideVerdict};
use super::AutoCenter;

/// Warm the thumbnail cache around the page the glide just centered on: the
/// two before and eight after cover the next flick of scrolling.
fn prefetch_neighborhood(page: u32) {
    leptos::task::spawn_local(async move {
        for p in page.saturating_sub(2)..=page + 8 {
            pdf_engine::api::prefetch_thumb(p, THUMB_SCALE).await;
        }
    });
}

/// One frame after the panel opens: re-measure (the aside now has real
/// layout), then snap. Instant, not a glide — the reader should land
/// centered on open, not watch the grid slide there. The 0ms-timer path
/// this replaces was armed inside the effect, so the first viewport/page
/// echo cancelled it before it ever fired.
fn snap_to_page(virtualizer: Virtualizer, state: ReaderState, page: u32) {
    request_animation_frame(move || {
        virtualizer.remeasure_container();
        let vh = virtualizer.viewport().get_untracked().main;
        if vh <= 1.0 {
            return;
        }
        let aspect = state.document.page1_aspect_untracked();
        if let Some(target) = center_target(&virtualizer, page, aspect, vh) {
            virtualizer.scroll_to_offset(target, ScrollMode::Instant);
        }
    });
}

/// Everything the armed glide step needs, cloned out of [`AutoCenter`] so
/// the step closure owns its world and the wiring stays readable.
struct Glide {
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    virtualizer: Virtualizer,
    last_user_drive: Rc<Cell<f64>>,
    timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    step_slot: StoredValue<Option<Rc<dyn Fn()>>, LocalStorage>,
    page: u32,
    /// Aspect frozen at arming time (the tracked read happened in the
    /// effect run that armed this glide; the step must not re-subscribe).
    aspect: f64,
}

/// Arm (or re-arm) the debounced glide toward `page`'s centered position.
///
/// The step is self-cancelling via [`glide_verdict`]: it re-checks the
/// panel, the page and the target before firing, waits out the after-drive
/// grace, and only then scrolls and prefetches.
fn arm_glide(g: Glide) {
    let Glide {
        state,
        sidebar,
        virtualizer,
        last_user_drive,
        timer,
        step_slot,
        page,
        aspect,
    } = g;

    // The step closure keeps its own handle; the arming delay below still
    // reads through ours.
    let step_drive = last_user_drive.clone();
    let step: Rc<dyn Fn()> = Rc::new(move || {
        let since_drive = js_sys::Date::now() - step_drive.get();
        let vh = virtualizer.viewport().get_untracked().main;
        let cur = virtualizer.scroll_offset().get_untracked();
        let target = center_target(&virtualizer, page, aspect, vh);
        let verdict = glide_verdict(
            sidebar.get_untracked() == SidebarMode::Thumbs,
            state.viewer.page.get_untracked(),
            page,
            target,
            cur,
            since_drive,
        );
        match verdict {
            GlideVerdict::Cancel => timer.set_value(None),
            GlideVerdict::Hold(wait_ms) => {
                // Re-read the CURRENT step (a newer arming may have replaced
                // it) and re-arm under a fresh timer.
                let next = step_slot.get_value();
                let handle = next.and_then(|next| {
                    set_timeout_with_handle(
                        move || next(),
                        Duration::from_millis(wait_ms),
                    )
                    .ok()
                });
                timer.set_value(handle);
            }
            // Auto, not Instant: the glide is the settle path, and Auto
            // resolves to instant anyway when the distance is more than
            // two screenfuls. The reader's scroll switch can shorten that to
            // always-instant, which is the same landing without the ride.
            GlideVerdict::Fire(target) => {
                let mode = if state.viewer.motion.get_untracked().scroll_glide {
                    ScrollMode::Auto
                } else {
                    ScrollMode::Instant
                };
                virtualizer.scroll_to_offset(target, mode);
                timer.set_value(None);
                prefetch_neighborhood(page);
            }
        }
    });
    step_slot.set_value(Some(step.clone()));

    if let Some(handle) = timer.get_value() {
        handle.clear();
        timer.set_value(None);
    }
    let since_drive = js_sys::Date::now() - last_user_drive.get();
    let delay = glide_delay((since_drive < GRACE_MS).then_some(since_drive));
    let fire = step.clone();
    let handle = set_timeout_with_handle(move || fire(), Duration::from_millis(delay)).ok();
    timer.set_value(handle);
}

/// The "take me to where I am" gesture: a `pdfreader:reveal-active`
/// event (re-clicking the active sidebar tab) smooth-scrolls onto the
/// current page and hands the panel back to the reader.
pub(super) fn install_reveal_listener(
    auto: &AutoCenter,
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
) {
    let reveal_drive = auto.last_user_drive.clone();
    let v = auto.virtualizer.clone();
    Effect::new(move |_| {
        let reveal_drive = reveal_drive.clone();
        let v = v.clone();
        let handle = window_event_listener(
            leptos::ev::Custom::new(crate::events::REVEAL_ACTIVE_EVENT),
            move |_: web_sys::CustomEvent| {
                if sidebar.get_untracked() != SidebarMode::Thumbs {
                    return;
                }
                let vh = v.viewport().get_untracked().main;
                let page = state.viewer.page.get_untracked();
                let target = center_target(&v, page, state.document.page1_aspect(), vh);
                if let Some(target) = target {
                    reveal_drive.set(f64::NEG_INFINITY);
                    // "Take me to where I am" is a request to ARRIVE, and
                    // the smooth scroll that expresses it is an animation
                    // like any other: off, the rail lands there.
                    let mode = if state.viewer.motion.get_untracked().scroll_glide {
                        ScrollMode::Smooth
                    } else {
                        ScrollMode::Instant
                    };
                    v.scroll_to_offset(target, mode);
                }
            },
        );
        on_cleanup(move || handle.remove());
    });
}

/// The open/page-follow effect: snap on a fresh open, then glide after
/// the page signal moves (debounced, grace-aware).
pub(super) fn install_center_effect(
    auto: &AutoCenter,
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
) {
    let virtualizer = auto.virtualizer.clone();
    let centered = auto.centered;
    let last_user_drive = auto.last_user_drive.clone();
    let glide_timer = auto.glide_timer;
    let glide_step = auto.glide_step;

    Effect::new(move |_| {
        let in_thumbs = sidebar.get() == SidebarMode::Thumbs;
        let page = state.viewer.page.get();
        // Tracked: a real measurement writes the viewport signal, which
        // re-arms this effect — so deferring here is safe.
        let vh = virtualizer.viewport().get().main;
        let (was_open, _prev_page) = centered.get_value();
        if !in_thumbs {
            centered.set_value((false, 0));
            return;
        }
        // Not ready yet: the container isn't bound or the viewport is
        // still unmeasured (a zero-size element, or geometry that has
        // not been reported). Returning keeps `centered = (false, _)`,
        // so the run that follows a real measurement is treated as a
        // fresh open and snaps — instead of silently dropping a scroll
        // against a placeholder viewport.
        if vh <= 1.0 {
            return;
        }
        if page == 0 {
            return;
        }

        let just_opened = !was_open;
        centered.set_value((true, page));

        if just_opened {
            snap_to_page(virtualizer.clone(), state, page);
            return;
        }

        let target = center_target(&virtualizer, page, state.document.page1_aspect(), vh);
        let Some(target) = target else {
            return;
        };
        let cur = virtualizer.scroll_offset().get_untracked();
        if (target - cur).abs() <= 1.0 {
            if let Some(handle) = glide_timer.get_value() {
                handle.clear();
                glide_timer.set_value(None);
            }
            return;
        }

        arm_glide(Glide {
            state,
            sidebar,
            virtualizer: virtualizer.clone(),
            last_user_drive: last_user_drive.clone(),
            timer: glide_timer,
            step_slot: glide_step,
            page,
            aspect: state.document.page1_aspect(),
        });
    });
}

/// Timer + step cleanup belongs to the panel's lifetime, not to effect
/// re-runs: a cleanup registered inside the effect would cancel the
/// armed glide on the next viewport/page echo — which is exactly what
/// killed the open snap. This runs only when the panel is disposed.
///
/// The order is load-bearing: the pending timer is cleared first (it may
/// still hold a clone of the step), then the step slot is dropped, so the
/// self-re-arming `Rc<dyn Fn()>` loses its last strong reference and the
/// glide cannot keep re-arming after the panel is gone.
pub(super) fn install_lifetime_cleanup(auto: &AutoCenter) {
    let timer = auto.glide_timer;
    let step_slot = auto.glide_step;
    on_cleanup(move || {
        if let Some(h) = timer.get_value() {
            h.clear();
            timer.set_value(None);
        }
        step_slot.set_value(None);
    });
}
