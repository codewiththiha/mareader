//! Asks the window for a frame, now or after a wait.
//!
//! A worker that finishes asks for a frame at once, which is what [`Waker`]
//! does. A controller that polls, or waits for a moment to arrive, has to ask
//! for a frame *later*, and a closure taking nothing cannot say when. This is
//! that second ask, so the waiting can live below the front end instead of the
//! view driving a tick it does not own.
//!
//! [`Waker`]: crate::session::Waker

use std::sync::Arc;
use std::time::Duration;

use crate::session::Waker;

/// Asks the window to draw again once `after` has passed.
pub type RepaintFn = Arc<dyn Fn(Duration) + Send + Sync>;

/// A window's repaints, as a controller sees them.
///
/// The shell builds one: a front end over its own context, a test over a
/// counter. It is cheap to clone, so a controller keeps its own and hands
/// copies to the threads it starts.
#[derive(Clone)]
pub struct Repaint {
    ask: RepaintFn,
}

impl Default for Repaint {
    fn default() -> Self {
        Self {
            ask: Arc::new(|_| {}),
        }
    }
}

impl Repaint {
    /// Keeps `ask`, which the window answers by drawing again.
    pub fn new(ask: RepaintFn) -> Self {
        Self { ask }
    }

    /// Draws again as soon as the window can.
    pub fn now(&self) {
        (self.ask)(Duration::ZERO);
    }

    /// Draws again once `after` has passed; a poll's tick, or a wait's end.
    pub fn after(&self, after: Duration) {
        (self.ask)(after);
    }

    /// The same ask as a [`Waker`], for a thread that finishes when it will.
    ///
    /// [`Waker`]: crate::session::Waker
    pub fn waker(&self) -> Waker {
        let ask = Arc::clone(&self.ask);
        Arc::new(move || ask(Duration::ZERO))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// The waits a test window was asked for, in order.
    fn recording() -> (Repaint, Arc<Mutex<Vec<Duration>>>) {
        let seen: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&seen);
        let repaint = Repaint::new(Arc::new(move |after| {
            if let Ok(mut log) = kept.lock() {
                log.push(after);
            }
        }));
        (repaint, seen)
    }

    /// What was asked for; a poisoned lock reads as nothing asked.
    fn asked(seen: &Mutex<Vec<Duration>>) -> Vec<Duration> {
        seen.lock().map_or_else(|_| Vec::new(), |l| l.clone())
    }

    #[test]
    fn now_asks_without_a_wait() {
        let (repaint, seen) = recording();
        repaint.now();
        assert_eq!(asked(&seen), vec![Duration::ZERO]);
    }

    #[test]
    fn after_asks_for_the_wait_it_was_given() {
        let (repaint, seen) = recording();
        repaint.after(Duration::from_millis(50));
        repaint.after(Duration::from_secs_f64(0.3));
        // No index: the engine denies indexing, its tests included.
        let asks = asked(&seen);
        assert_eq!(asks.len(), 2, "two asks, got {asks:?}");
        let tick = asks.first().map(Duration::as_secs_f64).unwrap_or(-1.0);
        let wait = asks.get(1).map(Duration::as_secs_f64).unwrap_or(-1.0);
        assert!((tick - 0.05).abs() < 0.001, "a tick of {tick}s");
        assert!((wait - 0.3).abs() < 0.01, "a wait of {wait}s");
    }

    #[test]
    fn a_clone_asks_the_same_window() {
        let (repaint, seen) = recording();
        let copy = repaint.clone();
        copy.now();
        repaint.now();
        assert_eq!(asked(&seen).len(), 2);
    }

    #[test]
    fn an_unattached_one_asks_nothing_and_fails_nothing() {
        Repaint::default().now();
        Repaint::default().after(Duration::from_secs(1));
    }
}
