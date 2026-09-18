//! Scroll-phase detection for the virtualized strips: how fast the reader is
//! moving, and in which of four regimes the move sits. The phase is what the
//! paint window, the engine's render gate and the strips' zombie retention
//! key on — one measurement, three consumers.
//!
//! The velocity is a MEDIAN of pairwise slopes over a short sample window,
//! not `Δoffset/Δt`: a single dropped frame spikes a naive slope 2-3×, and a
//! detector that flickers on one janky sample parks and unparks the renders
//! with it. Hysteresis (enter a fling high, leave it low) plus brake
//! detection (a deliberate direction reversal means the reader is catching
//! the fling and wants content fast) keep the phase from oscillating.
//!
//! The FSM itself is a pure function ([`next_phase`]) so the tests drive it
//! without a DOM; [`ScrollKinetics`] is the live half that attaches a
//! passive listener to a scroll container and publishes the phase as a
//! signal.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use leptos::prelude::*;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;

/// The four scroll regimes the reader can be in, ordered by the pressure
/// they put on the render pipeline (that rank is what merges the two axes
/// and the zoom into one phase).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScrollPhase {
    /// Still: deep prefetch, generous retention.
    Idle,
    /// Deliberate reading scroll: normal renders.
    Cruising,
    /// Decelerating after a fling: the settle timer is running.
    Settling,
    /// Fast scroll: real renders suspended, ghosts shown.
    Fling,
}

impl ScrollPhase {
    /// The higher-pressure of the two phases.
    pub fn max_pressure(self, other: ScrollPhase) -> ScrollPhase {
        if self.rank() >= other.rank() {
            self
        } else {
            other
        }
    }

    fn rank(self) -> u8 {
        match self {
            ScrollPhase::Idle => 0,
            ScrollPhase::Cruising => 1,
            ScrollPhase::Settling => 2,
            ScrollPhase::Fling => 3,
        }
    }
}

/// The thresholds that turn scroll speed into a phase.
///
/// Calibrated against this app's own inputs: the keyboard hold-scroll runs
/// at 1000 px/s (must stay Cruising — renders continue, as they should),
/// mouse-wheel spam lands at 800-1200 px/s (Cruising), and trackpad flings
/// hit 3000-10000 px/s (Fling). `fling_enter_v` sits cleanly between the
/// two input classes.
#[derive(Clone, Copy, Debug)]
pub struct KineticsConfig {
    /// px/s — enter Fling at or above this.
    pub fling_enter_v: f64,
    /// px/s — leave Fling at or below this (the hysteresis band's exit).
    pub fling_exit_v: f64,
    /// px/s — read as "stopped".
    pub idle_v: f64,
    /// ms of quiet before Settling → Idle (the "delay instead of render").
    pub settle_ms: f64,
    /// ms of quiet before Settling → Idle when the reader BRAKED (reversed
    /// direction while still moving): they are catching the fling, so
    /// content lands sooner.
    pub brake_ms: f64,
    /// How many recent (t_ms, offset) samples the velocity is taken over.
    pub sample_win: usize,
    /// ms — samples older than this (a pause mid-window) are discarded.
    pub fresh_ms: f64,
}

impl Default for KineticsConfig {
    fn default() -> Self {
        Self {
            fling_enter_v: 1800.0,
            fling_exit_v: 450.0,
            idle_v: 90.0,
            settle_ms: 140.0,
            brake_ms: 60.0,
            sample_win: 6,
            fresh_ms: 220.0,
        }
    }
}

/// Median of pairwise slopes over the window: robust to one janky frame.
/// `t` is milliseconds, `offset` the main-axis scroll position, so the
/// answer is px/s (the slopes are px/ms, scaled to the thresholds' units).
pub(crate) fn robust_velocity(samples: &[(f64, f64)]) -> f64 {
    let n = samples.len();
    if n < 2 {
        return 0.0;
    }
    let mut slopes = Vec::with_capacity(n * (n - 1) / 2);
    for i in 0..n {
        for j in (i + 1)..n {
            let dt = samples[j].0 - samples[i].0;
            if dt > 1.0 {
                slopes.push((samples[j].1 - samples[i].1) / dt * 1000.0);
            }
        }
    }
    if slopes.is_empty() {
        return 0.0;
    }
    slopes.sort_by(f64::total_cmp);
    slopes[slopes.len() / 2]
}

/// The hysteresis FSM, pure: the next phase given the current one, the
/// window's robust speed, whether the reader braked recently, and the
/// timestamp the speed last dropped to idle (`0.0` = not low yet). Returns
/// the new phase and the updated low-since timestamp.
pub(crate) fn next_phase(
    prev: ScrollPhase,
    speed: f64,
    braked: bool,
    low_since: f64,
    t: f64,
    cfg: &KineticsConfig,
) -> (ScrollPhase, f64) {
    let a = speed.abs();
    let mut low_since = low_since;
    let next = match prev {
        ScrollPhase::Idle | ScrollPhase::Cruising if a >= cfg.fling_enter_v => ScrollPhase::Fling,
        // A brake only ends a fling once its speed has subsided below the
        // enter threshold: a hard reversal INTO another fling keeps
        // flinging, and the brake's shortened settle applies from the
        // moment it actually slows.
        ScrollPhase::Fling
            if (braked && a < cfg.fling_enter_v) || a <= cfg.fling_exit_v => ScrollPhase::Settling,
        // Hysteresis band: inside a fling, speed that subsided but has not
        // reached the exit band holds the FLING — the reader is still
        // decelerating, and the renders stay parked until they really stop.
        ScrollPhase::Fling => ScrollPhase::Fling,
        ScrollPhase::Settling if a >= cfg.fling_enter_v => ScrollPhase::Fling,
        ScrollPhase::Settling => {
            if a <= cfg.idle_v {
                if low_since == 0.0 {
                    low_since = t;
                }
                let need = if braked { cfg.brake_ms } else { cfg.settle_ms };
                if t - low_since >= need {
                    low_since = 0.0;
                    ScrollPhase::Idle
                } else {
                    ScrollPhase::Settling
                }
            } else {
                low_since = 0.0;
                ScrollPhase::Settling
            }
        }
        _ if a > cfg.idle_v => ScrollPhase::Cruising,
        _ => ScrollPhase::Idle,
    };
    (next, low_since)
}

/// The container's scroll listener while it is attached: the element it is
/// on (so it can be removed) and the closure the element holds.
type ScrollBinding = (web_sys::Element, Closure<dyn Fn()>);

struct Inner {
    cfg: KineticsConfig,
    phase: RwSignal<ScrollPhase>,
    samples: RefCell<Vec<(f64, f64)>>,
    last_dir: Cell<f64>,
    brake_at: Cell<f64>,
    low_since: Cell<f64>,
    binding: RefCell<Option<ScrollBinding>>,
}

impl Inner {
    /// One (t_ms, offset) sample: the listener's whole job.
    fn sample(&self, t: f64, offset: f64) {
        {
            let mut s = self.samples.borrow_mut();
            if let Some(&(_, prev_o)) = s.last() {
                let d = offset - prev_o;
                if d.abs() > 0.5 {
                    let dir = d.signum();
                    // A reversal with substance is a brake: the reader is
                    // catching the fling, and the settle gets shorter.
                    if self.last_dir.get() != 0.0 && dir != self.last_dir.get() && d.abs() > 24.0 {
                        self.brake_at.set(t);
                    }
                    self.last_dir.set(dir);
                }
            }
            s.push((t, offset));
            while s.len() > self.cfg.sample_win {
                s.remove(0);
            }
            while s.len() > 2 && t - s[0].0 > self.cfg.fresh_ms {
                s.remove(0);
            }
        }
        let v = robust_velocity(&self.samples.borrow());
        let braked = t - self.brake_at.get() < 80.0;
        let (next, low_since) = next_phase(
            self.phase.get_untracked(),
            v,
            braked,
            self.low_since.get(),
            t,
            &self.cfg,
        );
        self.low_since.set(low_since);
        if next != self.phase.get_untracked() {
            self.phase.set(next);
        }
    }
}

/// One scroll container's phase tracker. Cheap to clone (Rc inside); attach
/// it to a container and read the phase through the signal for effects.
#[derive(Clone)]
pub struct ScrollKinetics(Rc<Inner>);

impl ScrollKinetics {
    pub fn new(cfg: KineticsConfig) -> Self {
        Self(Rc::new(Inner {
            cfg,
            phase: RwSignal::new(ScrollPhase::Idle),
            samples: RefCell::new(Vec::new()),
            last_dir: Cell::new(0.0),
            brake_at: Cell::new(0.0),
            low_since: Cell::new(0.0),
            binding: RefCell::new(None),
        }))
    }

    /// Attach a PASSIVE scroll listener to the container, replacing any
    /// previous attach so a re-bind never leaks a listener.
    pub fn attach(&self, el: &web_sys::Element) {
        self.detach();
        let inner = self.0.clone();
        // The closure takes its own clone: the binding bookkeeping below
        // still needs `inner` to remember the listener.
        let inner_cb = inner.clone();
        let el2 = el.clone();
        let cb = Closure::<dyn Fn()>::new(move || {
            let offset = (el2.scroll_top() + el2.scroll_left()) as f64;
            inner_cb.sample(js_sys::Date::now(), offset);
        });
        let opts = web_sys::AddEventListenerOptions::new();
        opts.set_passive(true);
        let _ = el.add_event_listener_with_callback_and_add_event_listener_options(
            "scroll",
            cb.as_ref().unchecked_ref(),
            &opts,
        );
        *inner.binding.borrow_mut() = Some((el.clone(), cb));
    }

    /// Remove the listener (a re-bind or an unmount).
    pub fn detach(&self) {
        if let Some((el, cb)) = self.0.binding.borrow_mut().take() {
            let _ = el.remove_event_listener_with_callback("scroll", cb.as_ref().unchecked_ref());
        }
    }

    /// The phase as a signal (for effects that react to transitions).
    pub fn phase_signal(&self) -> ReadSignal<ScrollPhase> {
        self.0.phase.read_only()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase_at(t: f64, speed: f64, braked: bool, low_since: f64, prev: ScrollPhase) -> (ScrollPhase, f64) {
        next_phase(prev, speed, braked, low_since, t, &KineticsConfig::default())
    }

    /// A 100ms gap in the middle of the window must not spike the velocity:
    /// the median of pairwise slopes stays near the true speed, where a
    /// naive Δoffset/Δt would jump to the gap's slope.
    #[test]
    fn the_median_slope_ignores_one_janky_frame() {
        let s = vec![(0.0, 0.0), (16.0, 30.0), (32.0, 60.0), (132.0, 90.0), (148.0, 120.0)];
        assert!(robust_velocity(&s).abs() < 900.0);
    }

    /// Enter a fling at 1800; inside the hysteresis band it must NOT leave;
    /// at the exit band it does.
    #[test]
    fn a_fling_needs_hysteresis_to_leave() {
        let (p, _) = phase_at(0.0, 1900.0, false, 0.0, ScrollPhase::Cruising);
        assert_eq!(p, ScrollPhase::Fling);
        let (p, _) = phase_at(100.0, 1000.0, false, 0.0, ScrollPhase::Fling);
        assert_eq!(p, ScrollPhase::Fling);
        let (p, _) = phase_at(200.0, 400.0, false, 0.0, ScrollPhase::Fling);
        assert_eq!(p, ScrollPhase::Settling);
    }

    /// Settling waits out the quiet window before it may idle.
    #[test]
    fn settling_waits_out_the_quiet_window_then_idles() {
        let (p, low) = phase_at(0.0, 0.0, false, 0.0, ScrollPhase::Fling);
        assert_eq!(p, ScrollPhase::Settling);
        let (p, low) = phase_at(100.0, 0.0, false, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Settling);
        // 150ms after the speed dropped to idle: 150 >= the 140ms window.
        let (p, low) = phase_at(250.0, 0.0, false, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Idle);
        assert_eq!(low, 0.0);
    }

    /// A braked settle needs only the shorter window (60ms, not 140).
    #[test]
    fn a_brake_shortens_the_settle() {
        let (p, low) = phase_at(0.0, 0.0, true, 0.0, ScrollPhase::Fling);
        assert_eq!(p, ScrollPhase::Settling);
        let (p, low) = phase_at(50.0, 0.0, true, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Settling); // 50 < 60
        let (p, _) = phase_at(120.0, 0.0, true, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Idle); // 70 >= 60
    }

    /// Motion during the quiet window restarts the clock.
    #[test]
    fn motion_during_the_settle_restarts_the_quiet_clock() {
        let (_, low) = phase_at(0.0, 0.0, false, 0.0, ScrollPhase::Fling);
        let (p, low) = phase_at(100.0, 0.0, false, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Settling);
        let (p, low) = phase_at(120.0, 200.0, false, low, ScrollPhase::Settling);
        assert_eq!(p, ScrollPhase::Settling);
        assert_eq!(low, 0.0);
    }

    /// A hard reversal into another fling's speed does not settle — the
    /// brake only ends a fling once its speed has subsided.
    #[test]
    fn a_brake_into_another_fling_keeps_flinging() {
        let (p, _) = phase_at(0.0, 3000.0, true, 0.0, ScrollPhase::Fling);
        assert_eq!(p, ScrollPhase::Fling);
    }
}
