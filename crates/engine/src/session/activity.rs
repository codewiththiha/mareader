//! The AI's work going on in every owner, gathered for the corner stack.

use super::Session;
use crate::activity::{self, Activity, Ending, Key, Owners};
use crate::notes_panel::page_of;
use crate::study::state::Key as Ask;

impl Session {
    /// Every job the AI works on now: the panes' first, then the library's.
    pub fn activities(&self) -> Vec<Activity> {
        let mut all = activity::of_study(self.study_mode());
        all.extend(self.explains());
        all.extend(activity::of_library(&self.library));
        all
    }

    /// How the job `key` names, now gone, ended.
    pub fn activity_ending(&self, key: Key) -> Ending {
        let owners = Owners {
            study: self.study_mode(),
            library: &self.library,
            words: &self.study,
        };
        activity::ending(owners, key)
    }

    /// The words the AI Dictionary explains, each at its mark's page.
    fn explains(&self) -> Vec<Activity> {
        let page = |pane, ask: Ask| {
            let Ask::Mark(id) = ask else {
                return None;
            };
            let mark = self.study.mark(&self.library.model, id)?;
            let place = self.documents.place(pane)?;
            page_of(self.documents.document(place.doc)?, mark.spot)
        };
        let asked = self.study.askers();
        let explain = |(ask, by): (Ask, &crate::study::state::Asker)| {
            activity::explain(by.pane, ask, &by.word, page(by.pane, ask))
        };
        let mut all: Vec<Activity> = asked.map(explain).collect();
        all.sort_by_key(|a| a.key);
        all
    }
}
