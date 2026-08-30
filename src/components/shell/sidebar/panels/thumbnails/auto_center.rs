//! Auto-center the current page in the thumbnail grid: the glide / grace /
//! debounce machinery.
//!
//! Scrolls through the panel's virtualizer — content coordinates,
//! layout-clamped — instead of hand-rolled offset arithmetic.
//!
//! Structure: `AutoCenter::install` only wires the three pieces together —
//! [`AutoCenter::install_reveal_listener`] (the "take me to where I am"
//! gesture), [`AutoCenter::install_center_effect`] (open-snap + follow), and
//! the panel-lifetime cleanup. The decisions each piece makes are pure
//! functions ([`center_offset`], [`glide_delay`], [`glide_verdict`]) so the
//! timing rules are named and unit-tested rather than buried in closures.

use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use leptos::prelude::*;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::ReaderState;
use crate::state::ui::SidebarMode;

use super::geometry::{CELL_W, GLIDE_DEBOUNCE_MS, GRACE_MS, THUMB_SCALE};

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
    pub glide_step: StoredValue<Option<Rc<dyn Fn()>>, LocalStorage>,
    /// The panel's virtualizer.
    pub virtualizer: Virtualizer,
}

/// Content-coordinate offset that vertically centers a row of height
/// `cell_h` whose top sits at `row_top`, in a viewport `vh` tall.
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

/// Initial delay before an armed glide fires.
///
/// Inside the after-drive grace the glide waits out the remainder plus a
/// beat (so a panel the reader just flicked doesn't start sliding under
/// them); outside it, the plain debounce applies. The open path never
/// reaches this — it snaps in [`snap_to_page`] — so there is no
/// "just opened" delay here.
fn glide_delay(in_grace_remaining_ms: Option<f64>) -> u64 {
    match in_grace_remaining_ms {
        Some(remaining) => (GRACE_MS - remaining + 60.0) as u64,
        None => GLIDE_DEBOUNCE_MS,
    }
}

/// One tick of the armed glide: what to do, given what changed since arming.
#[derive(Debug)]
enum GlideVerdict {
    /// The panel closed, the page moved on, or the target is already
    /// centered (within a px): cancel and drop the timer.
    Cancel,
    /// The reader drove the panel less than a grace period ago: hold for
    /// this many ms, then run the step again.
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
    if !in_thumbs
        || page_now != armed_page
        || target.is_none_or(|t| (t - current).abs() <= 1.0)
    {
        return GlideVerdict::Cancel;
    }
    if since_drive_ms < GRACE_MS {
        return GlideVerdict::Hold((GRACE_MS - since_drive_ms + 50.0) as u64);
    }
    GlideVerdict::Fire(target.unwrap())
}

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

impl AutoCenter {
    /// Create the bundle around the panel's virtualizer.
    pub fn new(virtualizer: Virtualizer) -> Self {
        Self {
            last_user_drive: Rc::new(Cell::new(f64::NEG_INFINITY)),
            centered: StoredValue::new_local((false, 0u32)),
            glide_timer: StoredValue::new_local(None::<TimeoutHandle>),
            glide_step: StoredValue::new_local(None::<Rc<dyn Fn()>>),
            virtualizer,
        }
    }

    /// Install the auto-center effects.
    pub fn install(self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        self.install_reveal_listener(state, sidebar);
        self.install_center_effect(state, sidebar);
        self.install_lifetime_cleanup();
    }

    /// The "take me to where I am" gesture: a `pdfreader:reveal-active`
    /// event (re-clicking the active sidebar tab) smooth-scrolls onto the
    /// current page and hands the panel back to the reader.
    fn install_reveal_listener(&self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        let reveal_drive = self.last_user_drive.clone();
        let v = self.virtualizer.clone();
        Effect::new(move |_| {
            let reveal_drive = reveal_drive.clone();
            let v = v.clone();
            let handle = window_event_listener(
                leptos::ev::Custom::new("pdfreader:reveal-active"),
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
    fn install_center_effect(&self, state: ReaderState, sidebar: RwSignal<SidebarMode>) {
        let virtualizer = self.virtualizer.clone();
        let centered = self.centered;
        let last_user_drive = self.last_user_drive.clone();
        let glide_timer = self.glide_timer;
        let glide_step = self.glide_step;

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

    /// Timer cleanup belongs to the panel's lifetime, not to effect
    /// re-runs: a cleanup registered inside the effect would cancel the
    /// armed glide on the next viewport/page echo — which is exactly what
    /// killed the open snap. This runs only when the panel is disposed.
    fn install_lifetime_cleanup(&self) {
        let timer = self.glide_timer;
        let step_slot = self.glide_step;
        on_cleanup(move || {
            if let Some(h) = timer.get_value() {
                h.clear();
                timer.set_value(None);
            }
            step_slot.set_value(None);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_offset_places_the_row_mid_viewport() {
        // A 100px row whose top sits at 200, in a 500px viewport: the scroll
        // offset that shows it centered is 200 + 50 - 250 = 0.
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
