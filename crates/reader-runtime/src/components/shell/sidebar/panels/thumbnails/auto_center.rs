//! Auto-center the current page in the thumbnail grid: the glide and
//! grace machinery.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use super::geometry::CELL_W;

/// The glide's debounce: fires this long after page writes settle.
const GLIDE_DEBOUNCE_MS: u64 = 80;
/// User-drive grace: auto-center defers this long after an interaction.
const GRACE_MS: f64 = 1500.0;
use crate::state::ReaderState;
use app_state::SidebarMode;

/// The armed glide step, parked where a re-arm can replace it mid-flight.
type GlideStep = Rc<dyn Fn()>;

/// Panel-lifetime state shared between the thumbnail panel's effects and the
/// auto-center machinery.
pub struct AutoCenter {
    /// Last time the user physically drove the thumb panel.
    pub last_user_drive: Rc<Cell<f64>>,
    /// (was-this-panel-open, last-centered page).
    pub centered: StoredValue<(bool, u32), LocalStorage>,
    /// Handle for the debounced auto-center glide.
    pub glide_timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    /// The current self-re-arming glide step.
    pub glide_step: StoredValue<Option<GlideStep>, LocalStorage>,
    pub virtualizer: Virtualizer,
}

impl AutoCenter {
    pub fn new(virtualizer: Virtualizer) -> Self {
        Self {
            last_user_drive: Rc::new(Cell::new(f64::NEG_INFINITY)),
            centered: StoredValue::new_local((false, 0u32)),
            glide_timer: StoredValue::new_local(None::<TimeoutHandle>),
            glide_step: StoredValue::new_local(None::<GlideStep>),
            virtualizer,
        }
    }

    /// Install the auto-center effects.
    pub fn install(self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        install_reveal_listener(&self, state, sidebar);
        install_center_effect(&self, state, sidebar);
        install_lifetime_cleanup(&self);
    }
}

/// The offset that vertically centers a row of height `cell_h`.
fn center_offset(row_top: f64, cell_h: f64, vh: f64) -> Option<f64> {
    (vh > 0.0).then(|| row_top + cell_h / 2.0 - vh / 2.0)
}

/// Content-coordinate target that vertically centers `page`'s cell.
fn center_target(v: &Virtualizer, page: u32, aspect: f64, vh: f64) -> Option<f64> {
    if page == 0 {
        return None;
    }
    let idx = (page - 1) as usize;
    center_offset(v.offset_of(idx), CELL_W * aspect, vh)
}

/// Delay before an armed glide fires: the grace remainder plus a
/// beat, else the debounce.
fn glide_delay(in_grace_remaining_ms: Option<f64>) -> u64 {
    match in_grace_remaining_ms {
        Some(remaining) => (GRACE_MS - remaining + 60.0) as u64,
        None => GLIDE_DEBOUNCE_MS,
    }
}

/// One tick of the armed glide: what to do, given what changed since arming.
#[derive(Debug)]
enum GlideVerdict {
    /// The panel closed, the page moved, or the target is centered.
    Cancel,
    /// The reader drove the panel recently: hold for this many ms.
    Hold(u64),
    /// Fire the glide onto this content offset.
    Fire(f64),
}

fn glide_verdict(
    in_thumbs: bool,
    page_now: u32,
    armed_page: u32,
    target: Option<f64>,
    current: f64,
    since_drive_ms: f64,
) -> GlideVerdict {
    if !in_thumbs || page_now != armed_page || target.is_none_or(|t| (t - current).abs() <= 1.0) {
        return GlideVerdict::Cancel;
    }
    if since_drive_ms < GRACE_MS {
        return GlideVerdict::Hold((GRACE_MS - since_drive_ms + 50.0) as u64);
    }
    GlideVerdict::Fire(target.unwrap())
}

/// Warm the cache around the centered page: two before, eight after.
fn prefetch_neighborhood(pane: crate::pane::handle::PaneHandle, page: u32) {
    crate::frame_pane::prefetch_thumbs(pane.id(), page.saturating_sub(2)..=page + 8);
}

/// One frame after the panel opens: re-measure, then snap — instant,
/// not a glide.
fn snap_to_page(virtualizer: Virtualizer, state: ReaderState, page: u32) {
    request_animation_frame(move || {
        // One frame later the panel or the reader can be gone.
        if virtualizer.viewport().try_get_untracked().is_none() {
            return;
        }
        if state.viewer.page.try_get_untracked().is_none() {
            return;
        }
        virtualizer.remeasure_container();
        let vh = virtualizer.viewport().get_untracked().main;
        if vh <= 1.0 {
            return;
        }
        let aspect = state.document.page1_aspect_now();
        if let Some(target) = center_target(&virtualizer, page, aspect, vh) {
            virtualizer.scroll_to_offset(target, ScrollMode::Instant);
        }
    });
}

/// Everything the glide step needs, cloned out of [`AutoCenter`].
struct Glide {
    state: ReaderState,
    sidebar: RwSignal<SidebarMode>,
    virtualizer: Virtualizer,
    last_user_drive: Rc<Cell<f64>>,
    timer: StoredValue<Option<TimeoutHandle>, LocalStorage>,
    step_slot: StoredValue<Option<GlideStep>, LocalStorage>,
    page: u32,
    /// Aspect frozen at arming; the step must not re-subscribe.
    aspect: f64,
}

/// Arm or re-arm the debounced glide toward `page`'s centered spot.
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
        // Timer fires can straggle past teardown; the reads below panic on
        // either purged owner.
        if virtualizer.viewport().try_get_untracked().is_none() {
            return;
        }
        if state.viewer.page.try_get_untracked().is_none() {
            return;
        }
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
            GlideVerdict::Cancel => {
                let _ = timer.try_set_value(None);
            }
            GlideVerdict::Hold(wait_ms) => {
                // Re-read the CURRENT step: a newer arming may replace it.
                let next = step_slot.try_get_value().flatten();
                let handle = next.and_then(|next| {
                    set_timeout_with_handle(move || next(), Duration::from_millis(wait_ms)).ok()
                });
                let _ = timer.try_set_value(handle);
            }
            // Auto, not Instant: the glide is the settle path; a reader switch
            // can shorten it.
            GlideVerdict::Fire(target) => {
                let mode = if state.viewer.motion.get_untracked().scroll_glide {
                    ScrollMode::Auto
                } else {
                    ScrollMode::Instant
                };
                virtualizer.scroll_to_offset(target, mode);
                let _ = timer.try_set_value(None);
                prefetch_neighborhood(state.pane, page);
            }
        }
    });
    let _ = step_slot.try_set_value(Some(step.clone()));

    if let Some(handle) = timer.try_get_value().flatten() {
        handle.clear();
        let _ = timer.try_set_value(None);
    }
    let since_drive = js_sys::Date::now() - last_user_drive.get();
    let delay = glide_delay((since_drive < GRACE_MS).then_some(since_drive));
    let fire = step.clone();
    let handle = set_timeout_with_handle(move || fire(), Duration::from_millis(delay)).ok();
    let _ = timer.try_set_value(handle);
}

/// Re-clicking the active tab scrolls onto the current page.
fn install_reveal_listener(auto: &AutoCenter, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
    let reveal_drive = auto.last_user_drive.clone();
    let v = auto.virtualizer.clone();
    Effect::new(move |_| {
        let reveal_drive = reveal_drive.clone();
        let v = v.clone();
        let handle = window_event_listener(
            leptos::ev::Custom::new(app_ui::events::REVEAL_ACTIVE_EVENT),
            move |_: web_sys::CustomEvent| {
                // Listeners go with their owner; the probe keeps reads safe.
                let Some(mode) = sidebar.try_get_untracked() else {
                    return;
                };
                if mode != SidebarMode::Thumbs {
                    return;
                }
                let Some(vp) = v.viewport().try_get_untracked() else {
                    return;
                };
                let vh = vp.main;
                let Some(page) = state.viewer.page.try_get_untracked() else {
                    return;
                };
                let target = center_target(&v, page, state.document.page1_aspect(), vh);
                if let Some(target) = target {
                    reveal_drive.set(f64::NEG_INFINITY);
                    // It asks to ARRIVE; the ride is optional.
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

/// The open/page-follow effect: snap, then glide.
fn install_center_effect(auto: &AutoCenter, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
    let virtualizer = auto.virtualizer.clone();
    let centered = auto.centered;
    let last_user_drive = auto.last_user_drive.clone();
    let glide_timer = auto.glide_timer;
    let glide_step = auto.glide_step;

    Effect::new(move |_| {
        // Tracks the LIVE viewport signal: a write during the close window
        // ends the run.
        let Some(page) = state.viewer.page.try_get() else {
            return;
        };
        let in_thumbs = sidebar.get() == SidebarMode::Thumbs;
        // Tracked: a real measurement re-arms this effect, so deferring is
        // safe.
        let vh = virtualizer.viewport().get().main;
        let (was_open, _prev_page) = centered.get_value();
        if !in_thumbs {
            centered.set_value((false, 0));
            return;
        }
        // Not ready yet: the run after a real measurement still snaps.
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

/// Timer and step cleanup, on the panel's lifetime only: order is
/// load-bearing.
fn install_lifetime_cleanup(auto: &AutoCenter) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_offset_places_the_row_mid_viewport() {
        // A 100px row at 200 in a 500px viewport centers at offset 0.
        assert_eq!(center_offset(200.0, 100.0, 500.0), Some(0.0));
        // Same row further down: the offset moves with the row's midpoint.
        assert_eq!(center_offset(1000.0, 100.0, 500.0), Some(800.0));
        // Unmeasured viewport: nothing to center against.
        assert_eq!(center_offset(200.0, 100.0, 0.0), None);
    }

    #[test]
    fn glide_delay_waits_out_the_grace_otherwise_debounces() {
        // Deep inside the grace: remainder plus the 60ms beat.
        assert_eq!(glide_delay(Some(100.0)), (GRACE_MS - 100.0 + 60.0) as u64);
        // At the grace edge the beat alone remains.
        assert_eq!(glide_delay(Some(GRACE_MS)), 60);
        // No recent drive: the plain debounce.
        assert_eq!(glide_delay(None), GLIDE_DEBOUNCE_MS);
    }

    #[test]
    fn glide_verdict_cancels_when_the_world_moved_on() {
        let target = Some(1000.0);
        // Panel closed.
        assert!(matches!(
            glide_verdict(false, 3, 3, target, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Page changed under us.
        assert!(matches!(
            glide_verdict(true, 4, 3, target, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Already centered within a pixel.
        assert!(matches!(
            glide_verdict(true, 3, 3, target, 999.5, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
        // Target gone (viewport unmeasured).
        assert!(matches!(
            glide_verdict(true, 3, 3, None, 0.0, GRACE_MS * 2.0),
            GlideVerdict::Cancel
        ));
    }

    #[test]
    fn glide_verdict_holds_inside_the_grace_and_fires_past_it() {
        // Inside the grace: hold for the remainder plus the 50ms beat.
        match glide_verdict(true, 3, 3, Some(1000.0), 0.0, 100.0) {
            GlideVerdict::Hold(wait) => assert_eq!(wait, (GRACE_MS - 100.0 + 50.0) as u64),
            other => panic!("expected Hold, got {other:?}"),
        }
        // Past the grace: fire onto the target.
        match glide_verdict(true, 3, 3, Some(1000.0), 0.0, GRACE_MS + 1.0) {
            GlideVerdict::Fire(t) => assert_eq!(t, 1000.0),
            other => panic!("expected Fire, got {other:?}"),
        }
    }
}
