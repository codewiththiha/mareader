//! Page-AI run lifecycle, result persistence, and queries on [`Ledger`].

use std::sync::Arc;
use std::time::Duration;

use mareader_library::{AiPageId, AiTask, BookId, Library, NewAiPage, PageSpan};
use mareader_study::page::Rewritten;
use mareader_study::{Failure, Why};
use mareader_workspace::PaneId;

use super::Slot;
use super::extract;
use super::layout::{anchor, pages_of};
use super::ledger::Ledger;
use super::state::{Asked, Key, News, Showing, canned, poll};
use super::view::source_of;

/// Seconds between settings of a result while it streams in.
const RESET: f64 = 0.4;

/// Why a page couldn't be done, in the reader's words.
fn say(f: &Failure) -> String {
    let why = match f.why {
        Why::NotHere => "No AI model is set up for this. Choose one in Settings › AI.",
        why => why.message(),
    };
    match &f.detail {
        Some(d) => format!("{why} ({d})"),
        None => why.to_owned(),
    }
}

impl Ledger {
    /// Writes a result's task again, replacing it once done.
    pub fn again(&mut self, asked: Asked<'_>, main: &Library, slot: Slot, now: f64) {
        let (task, old, span) = match slot {
            Slot::Saved(id) => match self.saved(main, id) {
                Some(p) => (p.task.clone(), Some(id), p.span.clone()),
                None => return,
            },
            Slot::Run(id) => match self.runs.iter().find(|r| r.id == id) {
                Some(r) => (r.task.clone(), r.redo_of, r.span.clone()),
                None => return,
            },
        };
        // A group is asked again whole, beside other groups under way.
        let group = asked.documents.ai_view(asked.pane).and_then(|view| {
            let (from, to) = pages_of(&view.document, &span)?;
            (to > from).then_some((view.layout, from, to))
        });
        if let Some((layout, from, to)) = group {
            if let Slot::Run(id) = slot {
                self.runs.retain(|r| r.id != id);
            }
            self.redo_group(asked, layout, (task, from, to), old);
            return;
        }
        self.redo(asked, task, old, now);
    }

    /// Takes in what the AI wrote; returns any failure notices to toast.
    pub fn receive(&mut self, main: &mut Library, now: f64) -> Vec<String> {
        let mut toasts = Vec::new();
        let mut done = Vec::new();
        let mut streaming = false;
        for run in &mut self.runs {
            for news in poll(run.handle.as_mut()) {
                match news {
                    News::Partial(r) => {
                        run.latest = Some(r);
                        run.generation += 1;
                    }
                    News::Done(Ok(r)) => {
                        run.handle = None;
                        done.push((run.id, r));
                    }
                    News::Done(Err(f)) => {
                        run.handle = None;
                        let batch = self
                            .batches
                            .values()
                            .find(|b| b.runs.iter().any(|(id, _)| *id == run.id));
                        // A busy model takes the group later, as slots free.
                        if f.why == Why::InUse && batch.is_some() {
                            run.requeue = true;
                            continue;
                        }
                        let mut said = say(&f);
                        if self.follows.values().any(|w| w.runs.contains(&run.id)) {
                            said.push_str(" Continuous mode stopped.");
                        }
                        if batch.is_some_and(|b| b.waiting_any()) {
                            said.push_str(" The pages left weren't asked.");
                        }
                        toasts.push(said.clone());
                        run.failed = Some(said);
                    }
                }
            }
            // A streaming result is set again only so often.
            if run.latest.is_some() && now - run.shown_at >= RESET {
                run.shown = run.latest.take().map(Arc::new);
                run.shown_at = now;
            }
            if run.latest.is_some() {
                streaming = true;
            }
        }
        if streaming {
            self.wait_after(Duration::from_secs_f64(RESET));
        }
        // A failed run that was shown leaves the page as it was.
        let failed: Vec<u64> = self
            .runs
            .iter()
            .filter(|r| r.failed.is_some())
            .map(|r| r.id)
            .collect();
        self.showing
            .retain(|_, s| !matches!(s.slot, Slot::Run(id) if failed.contains(&id)));
        for (id, result) in done {
            self.keep(main, id, result);
        }
        toasts
    }

    /// Saves run `id`'s result; whatever showed it shows the saved one.
    fn keep(&mut self, main: &mut Library, id: u64, result: Rewritten) {
        let Some(i) = self.runs.iter().position(|r| r.id == id) else {
            return;
        };
        let run = self.runs.remove(i);
        let new = NewAiPage {
            task: run.task.clone(),
            span: run.span.clone(),
            source: run.source.key(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            by: run.by.clone(),
        };
        let wall = crate::library::wall_ms();
        let (lib, book) = self.shelf(main, run.book, &run.name);
        let Some(saved) = lib.add_ai_page(book, new, wall) else {
            return;
        };
        if let Some(old) = run.redo_of.filter(|o| *o != saved) {
            lib.set_ai_pages_alive(&[old], false, wall);
        }
        self.parsed.insert(saved, Some(Arc::new(result)));
        match run.span {
            PageSpan::Blocks { from, to } => {
                self.spans.insert(saved, (anchor(from), anchor(to)));
            }
            PageSpan::Page { page } => {
                self.covers.insert(saved, (page, page));
            }
            PageSpan::Pages { from, to } => {
                self.covers.insert(saved, (from, to));
            }
        }
        let was = |s: &Showing| {
            s.slot == Slot::Run(id) || run.redo_of.is_some_and(|o| s.slot == Slot::Saved(o))
        };
        let keys: Vec<(Key, f64)> = self
            .showing
            .iter()
            .filter(|(_, s)| was(s))
            .map(|(k, s)| (*k, s.since))
            .collect();
        for (key, since) in keys {
            self.show_on(key, Slot::Saved(saved), since);
        }
    }

    /// The bubble `index` on page `page`: its layout, slot and own page.
    pub fn slot_at(&self, pane: PaneId, page: usize, index: usize) -> Option<(u64, Slot, usize)> {
        let (layout, pages) = self.drawn.get(&pane)?;
        let slots = &pages.iter().find(|(p, _)| *p == page)?.1;
        let (slot, own) = *slots.get(index)?;
        Some((*layout, slot, own))
    }

    /// Toggles the bubble `index`; returns `Some(slot)` if it failed.
    pub(crate) fn pick_slot(
        &mut self,
        pane: PaneId,
        page: usize,
        index: usize,
        now: f64,
    ) -> Option<Slot> {
        let (layout, slot, own) = self.slot_at(pane, page, index)?;
        let failed = match slot {
            Slot::Run(id) => self.runs.iter().any(|r| r.id == id && r.failed.is_some()),
            Slot::Saved(_) => false,
        };
        if failed {
            return Some(slot);
        }
        let key = (pane, layout, own);
        match self.showing.get(&key) {
            Some(s) if s.slot == slot => {
                self.showing.remove(&key);
            }
            _ => self.show_on(key, slot, now),
        }
        None
    }

    /// Shows `slot` over the page, or the page itself.
    pub(crate) fn show_result(&mut self, pane: PaneId, page: usize, slot: Option<Slot>, now: f64) {
        let Some((layout, pages)) = self.drawn.get(&pane) else {
            return;
        };
        let layout = *layout;
        let slots = pages
            .iter()
            .find(|(p, _)| *p == page)
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        match slot {
            Some(slot) => {
                if let Some((_, own)) = slots.iter().find(|(s, _)| *s == slot) {
                    self.show_on((pane, layout, *own), slot, now);
                }
            }
            None => {
                for (_, own) in slots {
                    self.showing.remove(&(pane, layout, own));
                }
            }
        }
    }

    /// A result's text, for the clipboard.
    pub fn text_of(&mut self, main: &Library, slot: Slot) -> Option<String> {
        let result = match slot {
            Slot::Saved(id) => self.result(main, id)?,
            Slot::Run(id) => self.runs.iter().find(|r| r.id == id)?.shown.clone()?,
        };
        Some(extract::plain(&result))
    }

    /// Deletes a kept result, or stops one being written.
    pub fn delete(&mut self, main: &mut Library, slot: Slot) {
        match slot {
            Slot::Saved(id) => {
                let wall = crate::library::wall_ms();
                if main.set_ai_pages_alive(&[id], false, wall) == 0 {
                    self.loose.0.set_ai_pages_alive(&[id], false, wall);
                }
                self.forget(id);
                self.runs.retain(|r| r.redo_of != Some(id));
            }
            Slot::Run(id) => self.runs.retain(|r| r.id != id),
        }
        self.showing.retain(|_, s| s.slot != slot);
    }

    /// Drops a gone result's text from every cache.
    fn forget(&mut self, id: AiPageId) {
        self.parsed.remove(&id);
        self.made.remove(&id);
        self.spans.remove(&id);
        self.covers.remove(&id);
    }

    /// Takes down results deleted or erased elsewhere, text and all.
    pub fn prune(&mut self, main: &Library) {
        let gone: Vec<AiPageId> = self
            .showing
            .values()
            .filter_map(|s| match s.slot {
                Slot::Saved(id) => self.saved(main, id).is_none().then_some(id),
                Slot::Run(_) => None,
            })
            .collect();
        for id in gone {
            self.forget(id);
            self.showing.retain(|_, s| s.slot != Slot::Saved(id));
        }
    }

    /// Forgets a closed pane: what it showed, drew and followed.
    pub fn close_pane(&mut self, pane: PaneId) {
        self.stop_follow(pane);
        self.stop_groups(pane);
        self.showing.retain(|(p, ..), _| *p != pane);
        self.drawn.remove(&pane);
        self.aligned.remove(&pane);
        self.wants.remove(&pane);
    }

    /// Shots and tests: keeps and shows a result; false before text.
    pub fn seed(&mut self, asked: Asked<'_>, main: &mut Library, task: AiTask, now: f64) -> bool {
        let Some(view) = asked.documents.ai_view(asked.pane) else {
            return false;
        };
        let page = view.document.to_base(asked.page);
        let Ok((source, span)) = source_of(&view, asked.documents, page) else {
            return false;
        };
        let result = canned(&source);
        let new = NewAiPage {
            task,
            span,
            source: source.key(),
            result: serde_json::to_string(&result).unwrap_or_default(),
            by: "Canned · test".to_owned(),
        };
        let wall = crate::library::wall_ms();
        let (lib, book) = self.shelf(main, asked.book, asked.name);
        let Some(id) = lib.add_ai_page(book, new, wall) else {
            return false;
        };
        let slot = Slot::Saved(id);
        let key = (asked.pane, view.layout, page);
        self.showing.insert(key, Showing { slot, since: now });
        true
    }

    /// Whether `pane` shows a result over a page on screen now.
    pub fn on_screen(&self, pane: PaneId) -> bool {
        let Some((layout, pages)) = self.drawn.get(&pane) else {
            return false;
        };
        let mut slots = pages.iter().flat_map(|(_, slots)| slots);
        slots.any(|(_, own)| self.showing.contains_key(&(pane, *layout, *own)))
    }

    /// Tests: the kept results' tasks and spans, oldest first.
    pub fn kept(&self, main: &Library, who: (Option<BookId>, &str)) -> Vec<(AiTask, PageSpan)> {
        let (lib, book) = self.lib(main, who.0, who.1);
        lib.ai_pages(book)
            .into_iter()
            .map(|p| (p.task.clone(), p.span.clone()))
            .collect()
    }

    /// Whether `pane` shows a result over `page`.
    pub fn shows(&self, pane: PaneId, page: usize) -> bool {
        self.showing_on(pane, page).is_some()
    }

    /// Whether the page shows a result in `pane`, for the menu.
    pub fn showing_on(&self, pane: PaneId, page: usize) -> Option<Slot> {
        let (layout, pages) = self.drawn.get(&pane)?;
        let Some((_, slots)) = pages.iter().find(|(p, _)| *p == page) else {
            // Off screen: a book's pages there are taken as its own.
            let shown = self.showing.get(&(pane, *layout, page));
            return shown.map(|s| s.slot);
        };
        slots.iter().find_map(|(_, own)| {
            let shown = self.showing.get(&(pane, *layout, *own))?;
            Some(shown.slot)
        })
    }
}
