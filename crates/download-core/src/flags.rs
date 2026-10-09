//! The two stop signals a running transfer polls.

use std::sync::atomic::{AtomicBool, Ordering};

use crate::Phase;

/// A pause and a cancel: read by the transfer, written from any thread.
#[derive(Debug, Default)]
pub(crate) struct Flags {
    cancel: AtomicBool,
    pause: AtomicBool,
}

impl Flags {
    pub(crate) fn pause(&self) {
        self.pause.store(true, Ordering::SeqCst);
    }

    pub(crate) fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Lift a pause; a cancel is never lifted.
    pub(crate) fn release(&self) {
        self.pause.store(false, Ordering::SeqCst);
    }

    /// The phase a stop asks for; a cancel outranks a pause.
    pub(crate) fn stop(&self) -> Option<Phase> {
        if self.cancel.load(Ordering::SeqCst) {
            Some(Phase::Cancelled)
        } else if self.pause.load(Ordering::SeqCst) {
            Some(Phase::Paused)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cancel_outranks_a_pause() {
        let flags = Flags::default();
        assert_eq!(flags.stop(), None);
        flags.pause();
        assert_eq!(flags.stop(), Some(Phase::Paused));
        flags.cancel();
        assert_eq!(flags.stop(), Some(Phase::Cancelled));
        // Releasing a pause cannot un-ask for a cancel.
        flags.release();
        assert_eq!(flags.stop(), Some(Phase::Cancelled));
    }
}
