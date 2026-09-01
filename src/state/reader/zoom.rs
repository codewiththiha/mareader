//! The zoom pipeline's reactive half: the command queue, the in-flight
//! transaction, and the three scales the controller moves.
//!
//! Nothing here decides anything — the resolving, tweening and committing all
//! live in `crate::viewer::zoom`. This is the shape those parts agree on.

use leptos::prelude::*;

/// One zoom intent, posted by whichever surface wants the zoom to change
/// (toolbar buttons, keyboard steps, the fit watcher, the follow watcher).
///
/// The [`crate::viewer::zoom::ZoomController`] is the only consumer: a
/// command is resolved against the current window, mode and page, and lands
/// through the one transition pipeline. Nobody executes a zoom by writing
/// the scale signals directly.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomCommand {
    /// One step along the preset ladder: `+1` zooms in, `-1` zooms out.
    Step(i32),
    /// Re-resolve the active fit mode (width / page) against the current
    /// window, view mode and page. Stands down when no fit mode is active.
    Refit,
    /// Re-resolve a manual zoom against the current window. The reader's
    /// chosen `desired` is authoritative up to the clamp: a manual zoom is
    /// never shrunk back to the fit width, so a page the reader zoomed in on
    /// stays at that scale (and overflows with a scroll affordance, rather
    /// than being cropped by a size the app chose).
    Constrain,
    /// The space around the page moved — a sidebar slide or a window drag.
    /// Resolves to whichever of the two above owns the scale (a fit mode when
    /// one is active, the reader's own `desired` when they zoomed by hand) and
    /// is posted on EVERY frame of the burst, because a scale that
    /// waits for the burst to end leaves the host wider than the box it now
    /// has to fit in and the flex engine squishes the paper. Its geometry
    /// lands in the frame it was asked for; its crisp render is held until the
    /// container has been quiet, so the burst costs one raster pass.
    Follow,
}

/// A live zoom transaction: what is animating, from where, to where. Exists
/// for exactly the duration of the transition; `None` means idle.
///
/// There is deliberately no position in here. The layout is rescaled on
/// every frame of the tween and the engine holds the document point under
/// the viewport centre exactly where it is, so a transaction has nothing to
/// remember about where the reader was looking.
#[derive(Debug, Clone, Copy)]
pub struct ZoomTransition {
    /// Visual scale the tween started from, so a retarget continues from
    /// wherever the eye currently is instead of teleporting.
    pub from: f64,
    /// Resolved target scale.
    pub to: f64,
    /// `Date::now()` at (re)targeting; a retarget restarts the clock.
    pub start_ms: f64,
    /// Whether the visual scale should tween; `false` lands on the first frame.
    pub animate: bool,
    /// True while this is a container [`ZoomCommand::Follow`] transaction. The
    /// distinction is load-bearing twice over: a follow's commit is HELD (the
    /// controller lands its geometry in the frame the size was reported and the
    /// settle deadline renders once the burst stops), and a watcher may only
    /// retarget a transaction of this kind — never a gesture's tween.
    pub following: bool,
}

/// The zoom pipeline scales, one type so they cannot drift apart across
/// modules. Three absolute scales, no ratios:
///
/// - `desired` is what the reader asked for, independent of whether it
///   currently fits (it is the ceiling a manual zoom resolves to, and the
///   readout tooltip explains it).
/// - `display` is the live visual scale — the scale the reader is looking at
///   right now. It moves on every frame of a zoom, and it is what the readout,
///   the fit maths, the page hosts and the overlays read.
/// - `committed` is the scale the mounted rasters are crisp at. It jumps
///   exactly once per zoom transaction, when the transition commits, and it
///   is the only scale a page render is issued at.
///
/// A fourth signal, `transition`, carries the in-flight transaction (and its
/// absence is what "not zooming" means). Commands queue on `commands`; the
/// controller is their only consumer.
#[derive(Clone, Copy)]
pub struct ZoomState {
    /// The zoom the reader asked for, independent of whether it currently
    /// fits the window.
    pub desired: RwSignal<f64>,
    /// The live visual scale. Moves every frame of a zoom, and the layout
    /// relayouts to it as it moves.
    pub display: RwSignal<f64>,
    /// The scale the mounted rasters are crisp at (page renders). Changes
    /// once per zoom transaction.
    pub committed: RwSignal<f64>,
    /// The in-flight transition, if any. While present, page/scroll
    /// synchronisation and geometry feedback are frozen.
    pub transition: RwSignal<Option<ZoomTransition>>,
    /// `(command, animate, token)` — the token makes every post unique, so
    /// two identical steps in a row both land.
    pub commands: RwSignal<Option<(ZoomCommand, bool, u64)>>,
    /// Monotonic command counter backing the token above.
    pub seq: RwSignal<u64>,
}

impl ZoomState {
    /// Post a zoom intent to the controller. `animate` asks for the eased
    /// tween; callers passing `false` get a first-frame landing.
    pub fn post(&self, cmd: ZoomCommand, animate: bool) {
        let token = self.seq.get_untracked() + 1;
        self.seq.set(token);
        self.commands.set(Some((cmd, animate, token)));
    }

    /// The live visual scale, read once and non-reactively.
    ///
    /// Named rather than reached for as `zoom.display.get_untracked()`
    /// because the three scales are all plain `f64` and reading the wrong one
    /// is a silent bug: `display` is what the reader is LOOKING at this frame,
    /// `committed` is what the mounted rasters are crisp at, and during a zoom
    /// they disagree. (Newtypes were the other option, but these scales cross
    /// into layout maths, the DOM and the engine on every frame, so wrapping
    /// them would have bought type safety at the cost of unwrapping at every
    /// use — the naming is where the distinction earns its keep.)
    pub fn visual_scale(&self) -> f64 {
        self.display.get_untracked()
    }

    /// The scale the in-flight transition is heading to, if any. Manual
    /// steps chain from this so a fast `+ +` advances two presets.
    pub fn in_flight_target(&self) -> Option<f64> {
        self.transition.get_untracked().map(|t| t.to)
    }

    /// Seed every scale for a freshly opened document: no transition, no
    /// layout to animate from, the live scale and the rasters already in
    /// agreement.
    pub fn initialize(&self, scale: f64) {
        self.desired.set(scale);
        self.display.set(scale);
        self.committed.set(scale);
        self.transition.set(None);
    }
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            desired: RwSignal::new(1.0),
            display: RwSignal::new(1.0),
            committed: RwSignal::new(1.0),
            transition: RwSignal::new(None),
            commands: RwSignal::new(None),
            seq: RwSignal::new(0),
        }
    }
}
