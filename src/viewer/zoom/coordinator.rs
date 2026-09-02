//! [`ZoomController`]: the single authority for what the effective zoom is
//! and how every change of it runs.
//!
//! All zoom traffic arrives as commands on `viewer.zoom.commands` — toolbar
//! buttons, keyboard steps, the fit watcher, the follow watcher — and is
//! executed by exactly one effect here. The controller:
//!
//! 1. resolves the command to a target (fit and constraint maths live in
//!    super::target),
//! 2. opens a transition from the scale on screen right now to that target,
//! 3. tweens the live display scale (super::animation), relaying the layout
//!    out through the engine on every frame so the document resizes
//!    continuously under the reader's eyes,
//! 4. and on landing brings the render scale onto the target and releases
//!    the freezes (render suspension, page/scroll synchronisation, geometry
//!    feedback, scroll echo).
//!
//! A container follow (`ZoomCommand::Follow`, one post per frame of a sidebar
//! slide or a window drag) is the one command that does not commit when it
//! lands. Its layout must move every frame or the page squishes, but a raster
//! pass per frame would be a storm, so the transition stays open — holding the
//! same freezes a gesture holds — and a deadline `FOLLOW_SETTLE_MS` after the
//! burst goes quiet commits it once. Every post moves the deadline, so what the
//! reader sees is a page that rides the slide and sharpens when it stops.
//!
//! Around the transaction, the strips' zombie retention grace is raised so
//! pages a moving window evicts keep their DOM (and their last bitmap)
//! briefly — the bridge that keeps the zoom from popping pages out.
//!
//! Because the controller is created with the reader page and lives exactly
//! as long as its reactive owner, there is no global registry to leak or
//! race: the old thread-local `CUR_ZOOM` registration is gone, and the
//! free-function `request_zoom` entry points with it.

use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::use_timeout::use_debounce;
use crate::state::reader::{ReaderState, ZoomTransition};
use crate::viewer::engine::ViewerEngine;

use super::animation::{land, Tween};
use super::command::holds_commit;
use super::config;
use super::target;

/// The one zoom authority. `Clone` so it can be handed out by value; it
/// holds nothing but the engine reference.
#[derive(Clone)]
pub struct ZoomController {
    engine: ViewerEngine,
}

impl ZoomController {
    pub fn new(engine: ViewerEngine) -> Self {
        Self { engine }
    }

    /// Start the command consumer and the freeze bookkeeping. Called once
    /// from the reader shell.
    pub fn drive(&self, state: ReaderState) {
        let engine = self.engine.clone();

        // While a transition is in flight, three feedback loops must stand
        // down: the browser's per-frame scroll echo (stale by one frame —
        // adopting it would fight the rescale anchor), size reports from the
        // strips (the guard in `PageStrip` already refuses to report
        // mid-zoom; the suspension makes that airtight at the virtualizer
        // too), and — on landing — the flush order, because the last
        // relayout must finish before buffered measurements re-enter.
        //
        // The grace that bridges a commit's evictions outlives the transaction
        // that raised it, so it is released on a timer rather than on the
        // commit — and the timer belongs to THIS owner, not to whoever happened
        // to be running when the transaction landed. An owner-scoped debounce
        // gives that for free: a pending fire is cleared on cleanup, where an
        // unowned `set_timeout` would let a fire land on a disposed reader, and
        // re-arming postpones the reset instead of queueing a second one.
        let v = engine.vertical.clone();
        let hv = engine.horizontal.clone();
        let grace_v = v.clone();
        let grace_hv = hv.clone();
        let grace = use_debounce(
            Duration::from_millis(u64::from(config::ZOOM_GRACE_MS)),
            move || {
                // Lower the grace back to its scroll default AND drop the
                // zombies the zoom raised it for, in the same breath. The
                // zoom's retained pages sit on large bitmaps; the per-item
                // expiry timer can outlive the transaction, so this closes
                // the window and releases their surfaces right after the
                // commit instead of waiting for the next scroll.
                grace_v.reset_retention_grace();
                grace_hv.reset_retention_grace();
                grace_v.prune_retained_now();
                grace_hv.prune_retained_now();
            },
        );
        Effect::new(move |_| {
            if state.viewer.zoom.transition.get().is_some() {
                v.suspend_scroll_feedback();
                hv.suspend_scroll_feedback();
                v.suspend_measurements();
                hv.suspend_measurements();
                // A transaction owns the bridge now; the reset it wants is the
                // one it schedules at its own end.
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

        // The held commit's deadline: a burst re-arms ONE fire rather than
        // queueing one per frame, so the transaction commits once the container
        // has gone quiet — and the newest post always owns that fire, which is
        // what keeps a follow from stranding uncommitted (and the freezes with
        // it, so nothing would render until the next zoom).
        let settle = use_debounce(Duration::from_millis(config::FOLLOW_SETTLE_MS), move || {
            let zoom = state.viewer.zoom;
            if let Some(t) = zoom.transition.get_untracked() {
                // Only ever a follow: a transaction that was opened or
                // replaced in the meantime carries its own commit.
                if t.following {
                    finish_transition(&state, &t);
                }
            }
        });

        Effect::new(move |_| {
            let Some((cmd, animate, _token)) = state.viewer.zoom.commands.get() else {
                return;
            };
            // Resolve against the in-flight target so chained steps advance
            // one preset per press. All reads inside are untracked: this
            // effect subscribes to the command signal and nothing else.
            let Some(target) = target::resolve(&state, cmd, state.viewer.zoom.in_flight_target())
            else {
                return;
            };

            let zoom = state.viewer.zoom;
            // A follow rides the container, so its own commit is deferred. Move
            // the deadline BEFORE deciding the frame is a no-op: while a burst
            // is running the scale may well be pinned (a page clamped at the
            // minimum, a hand-picked zoom capped at `desired` as the window
            // widens), and a commit that lands on the frame the container is
            // still moving rasterises at a width the reader is already past.
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
            // `from` is the visual scale RIGHT NOW, so a retarget continues
            // from wherever the eye currently is instead of teleporting.
            // Nothing about POSITION is captured: the layout relayouts
            // continuously and the engine holds the reader's view still
            // itself, frame by frame.
            let mode = state.viewer.mode.get_untracked();
            let transition = ZoomTransition {
                from: display,
                to: target,
                start_ms: js_sys::Date::now(),
                // There is nothing to ease into when the target is whatever the
                // container now allows: a follow that tweened would chase the
                // window instead of sitting in it.
                animate: animate && !following,
                following,
            };
            // Bridge the relayouts before they happen: raise the strips'
            // zombie grace so pages the moving window evicts keep their DOM
            // past the animation's end.
            let retention = config::profile_for(mode).retention;
            engine.vertical.set_retention_grace(retention.grace_ms);
            engine.horizontal.set_retention_grace(retention.grace_ms);
            // The transition goes up BEFORE anything moves, because the frames
            // it holds are exactly the ones that must not feed a measurement or
            // the browser's scroll echo back into the layout being resized.
            zoom.transition.set(Some(transition));
            if following {
                // A follow lands HERE rather than on the next animation frame.
                // The browser runs `ResizeObserver` callbacks after that frame's
                // rAF callbacks, so a landing handed to the tween loop would be
                // painted one frame after the container shrank — and a page row
                // the flex engine may not resize is by then a few pixels wider
                // than the box it has to fit in, which reads as a scrollbar
                // flickering along the whole length of a drag. Landing in this
                // task puts the new size in the same frame as the new width,
                // which is what a continuous follow has to mean.
                land(&state, &engine, &transition);
            } else {
                tween.arm(state, engine.clone());
            }
        });
    }
}

/// Land a transition: bring the render scale onto the target and release the
/// freezes.
///
/// There is no geometry step left to run. The last tween frame (or the first
/// frame of an untweened landing) already relayed the layout out to exactly
/// the target, so all that is left is for the rasters to catch up with the
/// size the hosts are already showing.
///
/// Every transaction ends here, and only the calls differ: a tween and a
/// discrete refit commit on the frame they land, while a container follow is
/// committed by the settle deadline above — once per burst, at the size the
/// container stopped at. It schedules nothing, and that is deliberate: see the
/// note on the grace bridge at the end. Setting the scales is a no-op write when a follow has
/// been landing all along, so a held commit is quiet even when it moves nothing.
pub(crate) fn finish_transition(state: &ReaderState, t: &ZoomTransition) {
    state.viewer.zoom.committed.set(t.to);
    state.viewer.zoom.display.set(t.to);
    // Releasing the transition last is what un-freezes page/scroll sync and
    // geometry feedback — everything downstream re-runs against a settled
    // scale, never a half-landed one.
    state.viewer.zoom.transition.set(None);
    // Nothing renders inside a transaction; sweep the rasters now that the
    // render scale has moved.
    pdf_engine::api::sweep();
    // The raised zombie grace is NOT lowered here. The bridge timer in `drive`
    // does that one grace window later, from the effect that watches this very
    // signal — so this function only writes signals and returns, which is what
    // lets both of its callers (the settle deadline, and the tween loop out of
    // a rAF callback) reach it from outside any owner of their own.
}
