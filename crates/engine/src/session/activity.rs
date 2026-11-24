//! The AI's work going on in every owner, gathered for the corner stack.

use super::Session;
use crate::activity::{self, Activity, Key};

impl Session {
    /// Every job the AI works on now, pane by pane.
    pub fn activities(&self) -> Vec<Activity> {
        activity::of_study(self.study_mode())
    }

    /// Whether the job `key` names, now gone, landed; else it was stopped.
    pub fn activity_landed(&self, key: Key) -> bool {
        activity::landed(self.study_mode(), key)
    }
}
