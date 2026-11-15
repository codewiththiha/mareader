//! What the engine's own tests drive a job with, where a window would be.
//!
//! A job takes a [`Waker`] and gives back what it found over a channel, so a
//! test answers the one and waits on the other. [`waited`] waits on a
//! condition the way a frame would, five milliseconds at a time, rather than
//! trusting a count of frames: the work below is a thread, and when it lands
//! is not the test's to say.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::session::Waker;

/// How long a test waits on work started off the frame: 200 of 5 ms.
pub const TRIES: usize = 200;

/// The wait between two looks at work still going.
pub const WAIT: Duration = Duration::from_millis(5);

/// A [`Waker`] that counts the frames asked of it, and the count it keeps.
pub fn asked() -> (Waker, Arc<Mutex<usize>>) {
    let wakes = Arc::new(Mutex::new(0));
    let kept = Arc::clone(&wakes);
    let wake: Waker = Arc::new(move || {
        if let Ok(mut asks) = kept.lock() {
            *asks += 1;
        }
    });
    (wake, wakes)
}

/// The frames `kept` has been asked for.
pub fn wakes(kept: &Mutex<usize>) -> usize {
    kept.lock().map_or(0, |asks| *asks)
}

/// Waits for `met`, five milliseconds at a time, a second at most.
pub fn waited(met: impl Fn() -> bool) -> bool {
    for _ in 0..TRIES {
        if met() {
            return true;
        }
        std::thread::sleep(WAIT);
    }
    met()
}
