//! Session-level update dispatch, frame polling, and exit apply hooks.

use crate::event::Event;
use crate::toast::Toast;
use crate::updates::{Action, Outcome, State, UpdatePlatform};

use super::{Done, Session};

impl Session {
    /// Current update state for front-end modals and Settings › Updates.
    pub fn updates(&self) -> &State {
        &self.updates
    }

    /// Overrides the update platform (used with UI `platform=` and CLI mocks).
    pub fn set_update_platform(&mut self, platform: UpdatePlatform) {
        self.updates.set_platform(platform);
    }

    /// Cleans leftover `.old` binaries and old staged caches on desktop launch.
    pub fn cleanup_stale_updates(&self) {
        self.updates.cleanup_stale(self.host.as_ref());
    }

    pub(crate) fn run_update(&mut self, action: Action) -> Done {
        if matches!(action, Action::Restart) {
            self.flush();
        }
        let outcome = self.updates.run(
            action,
            &self.settings.updates,
            self.host.as_ref(),
            &self.waker,
        );
        self.finish_update(outcome)
    }

    pub(super) fn poll_updates(&mut self) -> bool {
        let outcome = self
            .updates
            .poll(&self.settings.updates, self.host.as_ref(), &self.waker);
        let mut dirty = outcome.changed;
        if outcome.pending_restart {
            let done = self.run_update(Action::Restart);
            dirty |= done.dirty;
        } else {
            dirty |= self.finish_update(outcome).dirty;
        }
        dirty
    }

    fn finish_update(&mut self, outcome: Outcome) -> Done {
        if let Some(msg) = outcome.toast {
            self.emit(Event::Toast(Toast::info(msg)));
        }
        if outcome.close_settings {
            self.emit(Event::CloseSettings);
        }
        if outcome.restart_ready {
            self.emit(Event::UpdateRestart);
        }
        if outcome.changed {
            Done::changed()
        } else {
            Done::quiet()
        }
    }
}
