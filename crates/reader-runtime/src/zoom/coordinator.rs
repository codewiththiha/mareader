//! `ZoomController`: the one authority for the effective zoom, fed by
//! commands.

use std::time::Duration;

use leptos::prelude::*;

use crate::state::{ReaderState, ZoomTransition};
use crate::zoom::actuator::ZoomActuator;
use app_chrome::hooks::use_timeout::use_debounce;
use virtual_list_leptos::RetentionPolicy;

use super::animation::{Tween, commit_instant, interpolates_now, land};
use super::command::holds_commit;
use super::{config, target};

/// The one zoom authority, holding nothing but the actuator.
#[derive(Clone)]
pub struct ZoomController {
    actuator: ZoomActuator,
}

impl ZoomController {
    pub fn new(actuator: ZoomActuator) -> Self {
        Self { actuator }
    }

    /// Start the command consumer and the freeze bookkeeping. Called once
    /// from the reader shell.
    pub fn drive(&self, state: ReaderState) {
        let actuator = self.actuator.clone();

        // While a transition runs, three loops stand down: scroll echo, size
        // reports, flush order.
        let v = actuator.vertical.clone();
        let hv = actuator.horizontal.clone();
        #[cfg(feature = "pdf")]
        let pane = state.pane;
        let grace_v = v.clone();
        let grace_hv = hv.clone();
        let grace = use_debounce(
            Duration::from_millis(u64::from(config::ZOOM_GRACE_MS)),
            move || {
                // Lower the bridge to its scroll default AND drop the zombies.
                grace_v.reset_retention_policy();
                grace_hv.reset_retention_policy();
                grace_v.remove_retained_now();
                grace_hv.remove_retained_now();
                // The commit's renders have landed: drop the worker caches now.
                #[cfg(feature = "pdf")]
                {
                    let pdf = pane.pdf();
                    pdf.sweep();
                    pdf.sweep_snapshots();
                }
            },
        );
        Effect::new(move |_| {
            if state.viewer.zoom.transition.get().is_some() {
                v.suspend_scroll_feedback();
                hv.suspend_scroll_feedback();
                v.suspend_measurements();
                hv.suspend_measurements();
                // A transaction owns the bridge now; it resets its own way.
                grace.cancel();
            } else {
                v.resume_scroll_feedback();
                hv.resume_scroll_feedback();
                v.resume_measurements();
                hv.resume_measurements();
                grace.trigger();
            }
        });

        let tween = Tween::new();

        // The held commit's deadline: a burst re-arms ONE fire, so the
        // transaction commits once quiet.
        let settle = use_debounce(Duration::from_millis(config::FOLLOW_SETTLE_MS), move || {
            let zoom = state.viewer.zoom;
            // Only ever a follow: any other transaction carries its own commit.
            if let Some(t) = zoom.transition.get_untracked()
                && t.following
            {
                finish_transition(&state, &t);
            }
        });

        Effect::new(move |_| {
            let Some((cmd, animate, _token)) = state.viewer.zoom.commands.get() else {
                return;
            };
            // Resolve against the in-flight target so `+ +` chains presets.
            let Some(target) = target::resolve(&state, cmd, state.viewer.zoom.in_flight_target())
            else {
                return;
            };

            let zoom = state.viewer.zoom;
            // A follow's commit is deferred: move the deadline BEFORE any no-op
            // return.
            let following = holds_commit(cmd);
            if following {
                settle.trigger();
            }
            let display = zoom.visual_scale();
            let in_flight = zoom.transition.get_untracked();
            let settled = in_flight.map(|t| t.to).unwrap_or(display);
            if (target - settled).abs() < config::SETTLED_EPSILON {
                // Already there (or already heading there): nothing to move.
                return;
            }
            // `from` is the scale RIGHT NOW: a retarget continues from
            // there.
            let transition = ZoomTransition {
                from: display,
                to: target,
                start_ms: js_sys::Date::now(),
                // A follow has nothing to ease into: a tween would chase it.
                animate: animate && !following,
                following,
            };
            // Raise the zombie retention before the relayouts happen.
            let retention = config::zoom_profile().retention;
            let policy = RetentionPolicy::Grace {
                ms: retention.grace_ms,
                max: retention.max_zombies,
            };
            actuator.vertical.set_retention_policy(policy);
            actuator.horizontal.set_retention_policy(policy);
            if !following && !interpolates_now(&state, &transition) {
                // Animation off: one discrete change, no tween loop.
                commit_instant(&state, &actuator, &transition);
                return;
            }
            // The transition goes up BEFORE anything moves: its frames must not
            // feed back.
            zoom.transition.set(Some(transition));
            if following {
                // A follow lands HERE: a next-frame landing paints one late.
                land(&state, &actuator, &transition);
            } else {
                tween.arm(state, actuator.clone());
            }
        });
    }
}

/// Land a transition: bring the render scale onto the target and
/// release the freezes.
pub(crate) fn finish_transition(state: &ReaderState, t: &ZoomTransition) {
    state.viewer.zoom.committed.set(t.to);
    // A `set` notifies even for an equal value, so only write when the
    // display differs.
    if state.viewer.zoom.display.get_untracked() != t.to {
        state.viewer.zoom.display.set(t.to);
    }
    // Releasing the transition last un-freezes sync and geometry
    // feedback.
    state.viewer.zoom.transition.set(None);
    // Sweep the rasters now that the render scale has moved.
    #[cfg(feature = "pdf")]
    {
        let pdf = state.pane.pdf();
        pdf.sweep();
        pdf.sweep_snapshots();
    }
    // The heap probe at the commit: a zoom should read FLAT.
    app_state::memory::log_heap("zoom commit");
    // The raised grace is lowered by the bridge timer in `drive`, one
    // window later.
}
