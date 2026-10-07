//! The zoom pipeline's reactive half: the command queue, the in-flight
//! transaction, the three scales.

use leptos::prelude::*;

/// One zoom intent, posted by whatever surface wants the scale to
/// change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomCommand {
    /// One step along the preset ladder: `+1` zooms in, `-1` zooms out.
    Step(i32),
    /// Re-resolve the active fit mode against the window, mode and page.
    Refit,
    /// Re-resolve a manual zoom; the reader's `desired` is authoritative.
    Constrain,
    /// The space around the page moved; posted on EVERY frame of the burst.
    Follow,
}

/// A live zoom transaction: what is animating, from where, to where.
#[derive(Debug, Clone, Copy)]
pub struct ZoomTransition {
    /// The scale the tween started from, so a retarget continues from the
    /// eye.
    pub from: f64,
    pub to: f64,
    /// `Date::now()` at (re)targeting; a retarget restarts the clock.
    pub start_ms: f64,
    pub animate: bool,
    /// True for a container follow: its
    /// commit is HELD, retargetable by a
    /// watcher only.
    pub following: bool,
}

/// The three absolute scales, one type so they cannot drift apart.
#[derive(Clone, Copy)]
pub struct ZoomState {
    /// The zoom the reader asked for, independent of whether it currently
    /// fits the window.
    pub desired: RwSignal<f64>,
    /// The live visual scale, moved every frame of a zoom.
    pub display: RwSignal<f64>,
    /// The scale the mounted rasters are crisp at (page renders). Changes
    /// once per zoom transaction.
    pub committed: RwSignal<f64>,
    /// The in-flight transition, if any. While present, page/scroll
    /// synchronisation and geometry feedback are frozen.
    pub transition: RwSignal<Option<ZoomTransition>>,
    /// `(command, animate, token)`; the token makes every post unique.
    pub commands: RwSignal<Option<(ZoomCommand, bool, u64)>>,
    /// Monotonic command counter backing the token above.
    pub seq: RwSignal<u64>,
}

impl ZoomState {
    /// Post a zoom intent. `animate` asks for the eased tween.
    pub fn post(&self, cmd: ZoomCommand, animate: bool) {
        let token = self.seq.get_untracked() + 1;
        self.seq.set(token);
        self.commands.set(Some((cmd, animate, token)));
    }

    /// The live visual scale, read non-reactively: named because all three
    /// scales are `f64`.
    pub fn visual_scale(&self) -> f64 {
        self.display.get_untracked()
    }

    /// The scale the in-flight transition heads to; steps chain from it.
    pub fn in_flight_target(&self) -> Option<f64> {
        self.transition.get_untracked().map(|t| t.to)
    }

    /// Seed every scale for a freshly opened document.
    pub fn initialize(&self, scale: f64) {
        let Self {
            desired,
            display,
            committed,
            transition,
            // The queue is the controller's, not the document's.
            commands: _,
            seq: _,
        } = *self;
        desired.set(scale);
        display.set(scale);
        committed.set(scale);
        transition.set(None);
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
