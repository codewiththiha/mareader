//! The time left as shown: a clock running down, not redrawn each sentence.

/// Seconds a drifting estimate takes to pull the clock most of the way.
const SETTLE: f64 = 45.0;
/// The most the pull speeds or slows the clock, so minutes tick evenly.
const PULL: f64 = 0.5;
/// A move through the book this large, as a share, is a seek.
const LEAP: f32 = 0.004;
/// An estimate this far off the clock, as a share, is shown at once.
const OFF: f64 = 0.2;

/// The seconds shown, and what they were last set from.
#[derive(Debug, Default)]
pub struct Countdown {
    shown: Option<f64>,
    at: Option<f32>,
    rate: f32,
    time: Option<f64>,
}

/// One frame's estimate: seconds left, share read, playing, and speed.
#[derive(Clone, Copy, Debug)]
pub struct Estimate {
    pub left: Option<f64>,
    pub at: Option<f32>,
    pub playing: bool,
    pub rate: f32,
}

impl Countdown {
    /// The seconds to show at `now`.
    pub fn show(&mut self, e: Estimate, now: f64) -> Option<f64> {
        let Some(left) = e.left else {
            *self = Self::default();
            return None;
        };
        let dt = self.time.map_or(0.0, |t| (now - t).clamp(0.0, 1.0));
        self.time = Some(now);
        let leapt = matches!((self.at, e.at), (Some(a), Some(b)) if (a - b).abs() > LEAP);
        let rated = (self.rate - e.rate).abs() > 0.001;
        (self.at, self.rate) = (e.at, e.rate);
        let shown = match self.shown {
            // Seeks, a new speed, the end or a far-off guess show at once.
            Some(shown) if !leapt && !rated && left > 0.0 && near(shown, left) => {
                match e.playing {
                    // It runs with the voice, a little faster or slower.
                    true => {
                        let ticked = shown - dt;
                        let pull = (left - ticked) * (1.0 - (-dt / SETTLE).exp());
                        ticked + pull.clamp(-PULL * dt, PULL * dt)
                    }
                    false => shown,
                }
            }
            _ => left,
        };
        let shown = shown.max(0.0);
        self.shown = Some(shown);
        Some(shown)
    }
}

/// Whether `left` is close enough to ease toward instead of jump to.
fn near(shown: f64, left: f64) -> bool {
    (shown - left).abs() <= OFF * left.max(60.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn estimate(left: f64, at: f32, playing: bool) -> Estimate {
        Estimate {
            left: Some(left),
            at: Some(at),
            playing,
            rate: 1.0,
        }
    }

    #[test]
    fn a_wobbling_guess_counts_down_smoothly() {
        let mut c = Countdown::default();
        let mut last = f64::MAX;
        // A 100-minute book, its guess off by up to 8% each sentence.
        for frame in 0..6000 {
            let now = f64::from(frame) * 0.1;
            let sentence = (now / 5.0).floor();
            let wobble = [1.0, 1.08, 0.95, 1.05, 0.92][sentence as usize % 5];
            let left = (6000.0 - sentence * 5.0) * wobble;
            let at = (sentence * 5.0 / 6000.0) as f32;
            let shown = c.show(estimate(left, at, true), now).unwrap_or_default();
            if frame > 0 {
                // Between half and one and a half times the clock.
                let fell = last - shown;
                assert!((0.049..=0.151).contains(&fell), "{now}: {last} → {shown}");
            }
            last = shown;
        }
        assert!((last - 5400.0).abs() < 300.0, "kept near the clock: {last}");
    }

    #[test]
    fn seeks_speed_and_the_end_show_at_once() {
        let mut c = Countdown::default();
        c.show(estimate(3000.0, 0.5, true), 0.0);
        let seek = c.show(estimate(2800.0, 0.6, true), 0.1);
        assert_eq!(seek, Some(2800.0));
        let faster = Estimate {
            rate: 1.25,
            ..estimate(2700.0, 0.6, true)
        };
        assert_eq!(c.show(faster, 0.2), Some(2700.0));
        let end = Estimate {
            rate: 1.25,
            ..estimate(0.0, 1.0, false)
        };
        assert_eq!(c.show(end, 0.3), Some(0.0));
    }

    #[test]
    fn paused_it_holds_and_a_far_guess_replaces_it() {
        let mut c = Countdown::default();
        c.show(estimate(3000.0, 0.5, true), 0.0);
        assert_eq!(c.show(estimate(3100.0, 0.5, false), 0.5), Some(3000.0));
        assert_eq!(c.show(estimate(4000.0, 0.5, false), 1.0), Some(4000.0));
        assert_eq!(
            c.show(
                Estimate {
                    left: None,
                    ..estimate(0.0, 0.5, false)
                },
                1.5
            ),
            None
        );
    }
}
