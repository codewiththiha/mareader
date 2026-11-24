//! The AI's work going on in every owner, gathered for the corner stack.

use super::Session;
use crate::activity::{self, Activity, Ending, Key};

impl Session {
    /// Every job the AI works on now: the panes' first, then the library's.
    pub fn activities(&self) -> Vec<Activity> {
        let mut all = activity::of_study(self.study_mode());
        all.extend(activity::of_library(&self.library));
        all
    }

    /// How the job `key` names, now gone, ended.
    pub fn activity_ending(&self, key: Key) -> Ending {
        activity::ending(self.study_mode(), &self.library, key)
    }
}
