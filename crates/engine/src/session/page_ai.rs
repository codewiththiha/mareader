//! Page-AI ledger ownership and operations inside [`Session`].

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mareader_library::{AiPageId, AiTask, PageSpan};
use mareader_session::Inserts;
use mareader_study::page::Rewritten;
use mareader_workspace::PaneId;

use crate::event::Event;
use crate::page_ai::Slot;
use crate::page_ai::ledger::Ledger;
use crate::page_ai::state::{Around, Asked};
use crate::page_ai::view::{AiView, Models, Views};
use crate::toast::Toast;

use super::Session;

/// Aligned page-AI view and per-page result index for one overlay frame.
pub type PreparedOverlays = (AiView, Arc<HashMap<usize, Vec<AiPageId>>>);

impl Session {
    /// The session's page-AI ledger, read-only.
    pub fn page_ai(&self) -> &Ledger {
        &self.page_ai
    }

    /// Tests: runs answer at once, in capitals, with no AI.
    pub fn page_ai_set_canned(&mut self, canned: bool) {
        self.page_ai.canned = canned;
    }

    /// Tests and shots: runs show their words but never finish.
    pub fn page_ai_set_held(&mut self, held: bool) {
        self.page_ai.held = held;
    }

    /// Emits any queued page-AI notices as toast events.
    fn flush_page_ai_notes(&mut self) {
        for why in std::mem::take(&mut self.page_ai.notes) {
            self.emit(Event::Toast(Toast::info(why)));
        }
    }

    fn with_page_ai<R>(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        run: impl FnOnce(&mut Ledger, Asked<'_>, &mut mareader_library::Library) -> R,
    ) -> R {
        let book = self.book_in(pane);
        let name = self.documents.title(pane).unwrap_or_default().to_owned();
        let asked = Asked {
            pane,
            page,
            book,
            name: &name,
            documents: &self.documents,
            ai,
            settings: &self.settings.ai,
        };
        let out = run(&mut self.page_ai, asked, &mut self.library.model);
        self.flush_page_ai_notes();
        out
    }

    /// Starts or carries on `pane`'s continuous run.
    pub fn page_ai_follow(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        task: AiTask,
        fresh: bool,
        now: f64,
    ) {
        self.with_page_ai(pane, page, ai, |ledger, asked, main| {
            ledger.follow(asked, main, task, fresh, now);
        });
    }

    /// Carries a stopped continuous run on from where it was.
    pub fn page_ai_resume(&mut self, pane: PaneId, page: usize, ai: &dyn Models, now: f64) {
        self.with_page_ai(pane, page, ai, |ledger, asked, main| {
            ledger.resume(asked, main, now);
        });
    }

    /// Writes `task` for one page.
    pub fn page_ai_run(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        task: AiTask,
        now: f64,
    ) {
        self.with_page_ai(pane, page, ai, |ledger, asked, _| {
            ledger.redo(asked, task, None, now);
        });
    }

    /// Writes `task` over picked pages, in groups.
    pub fn page_ai_run_pages(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        pages: &[usize],
        task: AiTask,
    ) {
        self.with_page_ai(pane, page, ai, |ledger, asked, _| {
            ledger.run_pages(asked, pages, task);
        });
    }

    /// Writes a result's task again, replacing it once done.
    pub fn page_ai_again(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        slot: Slot,
        now: f64,
    ) {
        self.with_page_ai(pane, page, ai, |ledger, asked, main| {
            ledger.again(asked, main, slot, now);
        });
    }

    /// Ends `pane`'s continuous run.
    pub(crate) fn page_ai_stop_follow(&mut self, pane: PaneId) {
        self.page_ai.stop_follow(pane);
    }

    /// Drops `pane`'s waiting page groups and stops those being written.
    pub(crate) fn page_ai_stop_groups(&mut self, pane: PaneId) {
        self.page_ai.stop_groups(pane);
    }

    /// Stops whatever run `pane`'s pill represents.
    pub(crate) fn page_ai_stop_pill(&mut self, pane: PaneId) {
        self.page_ai.stop_pill(pane);
    }

    /// Shows `slot` over `page` in `pane`, or the original page if `None`.
    pub(crate) fn page_ai_show_result(
        &mut self,
        pane: PaneId,
        page: usize,
        slot: Option<Slot>,
        now: f64,
    ) {
        self.page_ai.show_result(pane, page, slot, now);
    }

    /// Copies a result's plain text to the host clipboard and notifies.
    pub(crate) fn page_ai_copy(&mut self, slot: Slot) {
        if let Some(text) = self.page_ai.text_of(&self.library.model, slot) {
            self.host.set_clipboard(text);
            self.emit(Event::Toast(Toast::info("Copied")));
        }
    }

    /// Deletes a kept result, or stops one being written.
    pub(crate) fn page_ai_delete(&mut self, slot: Slot) {
        self.page_ai.delete(&mut self.library.model, slot);
    }

    /// Polls running page-AI jobs, saves finished results, and emits notices.
    pub(crate) fn page_ai_receive(&mut self, now: f64) {
        for said in self.page_ai.receive(&mut self.library.model, now) {
            self.emit(Event::Toast(Toast::info(said)));
        }
    }

    /// Advances continuous and grouped page-AI runs; returns any poll wait.
    pub fn page_ai_tick(&mut self, ai: &dyn Models, now: f64) -> Option<Duration> {
        let around = Around {
            main: &self.library.model,
            documents: &self.documents,
            ai,
            settings: &self.settings.ai,
        };
        self.page_ai.tick(&around, now);
        self.flush_page_ai_notes();
        self.page_ai.take_wait()
    }

    /// Shots and tests: keeps and shows a canned result; false before text.
    pub fn page_ai_seed(
        &mut self,
        pane: PaneId,
        page: usize,
        ai: &dyn Models,
        task: AiTask,
        now: f64,
    ) -> bool {
        self.with_page_ai(pane, page, ai, |ledger, asked, main| {
            ledger.seed(asked, main, task, now)
        })
    }

    /// Kept results `pane`'s reflowable book lays in; none for a PDF.
    pub fn page_ai_inserts(&mut self, pane: PaneId) -> Option<Arc<Inserts>> {
        let book = self.book_in(pane);
        let name = self.documents.title(pane).unwrap_or_default();
        self.page_ai
            .inserts(&self.library.model, pane, &self.documents, (book, name))
    }

    /// Toggles bubble `index` on `page`; returns `Some(slot)` if it failed.
    pub fn page_ai_pick(
        &mut self,
        pane: PaneId,
        page: usize,
        index: usize,
        now: f64,
    ) -> Option<Slot> {
        self.page_ai.pick_slot(pane, page, index, now)
    }

    /// Prunes, aligns, and indexes `pane`'s results for one overlay frame.
    pub fn page_ai_prepare_overlays(&mut self, pane: PaneId) -> Option<PreparedOverlays> {
        let Some(view) = self.documents.ai_view(pane) else {
            self.page_ai.drawn.remove(&pane);
            return None;
        };
        let book = self.book_in(pane);
        let name = self.documents.title(pane).unwrap_or_default();
        let who = (book, name);
        let main = &self.library.model;
        self.page_ai.prune(main);
        self.page_ai.align(main, pane, &view, who);
        let index = self.page_ai.index(main, &view, who);
        Some((view, index))
    }

    /// Records the bubbles drawn on `pane` this frame for click hit-testing.
    pub fn page_ai_record_drawn(
        &mut self,
        pane: PaneId,
        layout: u64,
        drawn: Vec<(usize, Vec<(Slot, usize)>)>,
    ) {
        self.page_ai.drawn.insert(pane, (layout, drawn));
    }

    /// Parses and caches a saved result's rewritten body.
    pub fn page_ai_result(&mut self, id: AiPageId) -> Option<Arc<Rewritten>> {
        self.page_ai.result(&self.library.model, id)
    }

    /// Tests: whether `pane`'s `page` has readable source text for page AI.
    pub fn page_ai_can_read(&self, pane: PaneId, page: usize) -> bool {
        let view = self.documents.ai_view(pane);
        let at = |v: &AiView| v.document.to_base(page);
        view.is_some_and(|v| crate::page_ai::view::source_of(&v, &self.documents, at(&v)).is_ok())
    }

    /// Tests: the kept results' tasks and spans, oldest first.
    pub fn page_ai_kept(&self, pane: PaneId) -> Vec<(AiTask, PageSpan)> {
        let book = self.book_in(pane);
        let name = self.documents.title(pane).unwrap_or_default();
        self.page_ai.kept(&self.library.model, (book, name))
    }
}
