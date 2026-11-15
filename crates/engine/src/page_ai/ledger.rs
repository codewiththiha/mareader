//! What a page's AI keeps between frames, and names no window.
//!
//! The front end holds the pixels beside it: the cards a frame drew, the menus
//! and the places they open at. Everything here is a run, a result, or one
//! pane's book of them, so a test can drive a page's AI with no window at all.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use mareader_library::{AiPage, AiPageId, AiTask, BookId, Library, PageSpan};
use mareader_session::{Anchor, Document, Insert, Inserts};
use mareader_study::page::Rewritten;
use mareader_workspace::PaneId;

use super::Slot;
use super::extract::Source;
use super::follow::Follow;
use super::groups::Batch;
use super::state::{Asked, Cover, Drawn, Handle, Index, Key, Loose, Run, Showing, Who, canned};
use super::view::{page_task, source_of};
use crate::session::Waker;
use crate::study::loose::{loose_book, loose_id};

/// The page AI's state across panes.
pub struct Ledger {
    /// Wakes the client when background page-extraction threads finish.
    pub waker: Waker,
    /// Soonest deferred poll wait recorded by streaming or follow timers.
    wait: Option<Duration>,
    pub loose: Loose,
    pub runs: Vec<Run>,
    pub next_run: u64,
    pub showing: HashMap<Key, Showing>,
    /// By layout: a book's results laid in keep its own pages' places.
    pub index: HashMap<u64, Index>,
    /// The layout each pane's results were last counted in.
    pub aligned: HashMap<PaneId, u64>,
    /// Results as laid into a book's pages, by id.
    pub made: HashMap<AiPageId, Arc<Insert>>,
    /// Each book result's text, from where to where, as last indexed.
    pub spans: HashMap<AiPageId, (Anchor, Anchor)>,
    /// Each PDF result's first and last page, as last indexed.
    pub covers: HashMap<AiPageId, (usize, usize)>,
    /// What each pane's book was last asked to lay in.
    pub wants: HashMap<PaneId, Arc<Inserts>>,
    /// Results read once, by id.
    pub parsed: HashMap<AiPageId, Option<Arc<Rewritten>>>,
    /// Each pane's bubbles as last drawn, to tell which was pressed.
    pub drawn: HashMap<PaneId, Drawn>,
    /// Each pane's continuous run.
    pub follows: HashMap<PaneId, Follow>,
    /// Each pane's picked pages, asked in groups.
    pub batches: HashMap<PaneId, Batch>,
    /// What the groups have to say, for the next frame's toasts.
    pub notes: Vec<String>,
    /// Tests: runs answer at once with the page's own text.
    pub canned: bool,
    /// Tests and shots: runs stop at their words so far.
    pub held: bool,
    /// Tests: pages ahead are read on the frame.
    pub inline: bool,
}

impl Default for Ledger {
    fn default() -> Self {
        Self {
            waker: Arc::new(|| {}),
            wait: None,
            loose: Loose::default(),
            runs: Vec::new(),
            next_run: 0,
            showing: HashMap::new(),
            index: HashMap::new(),
            aligned: HashMap::new(),
            made: HashMap::new(),
            spans: HashMap::new(),
            covers: HashMap::new(),
            wants: HashMap::new(),
            parsed: HashMap::new(),
            drawn: HashMap::new(),
            follows: HashMap::new(),
            batches: HashMap::new(),
            notes: Vec::new(),
            canned: false,
            held: false,
            inline: false,
        }
    }
}

impl Ledger {
    /// Schedules the soonest deferred frame poll owed by this ledger.
    pub(crate) fn wait_after(&mut self, after: Duration) {
        self.wait = Some(self.wait.map_or(after, |w| w.min(after)));
    }

    /// Takes any deferred poll wait recorded since the last check.
    pub fn take_wait(&mut self) -> Option<Duration> {
        self.wait.take()
    }
    /// The book's library: the app's, else the session's for `name`.
    pub fn shelf<'a>(
        &'a mut self,
        main: &'a mut Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a mut Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => {
                let id = loose_book(&mut self.loose.0, name);
                (&mut self.loose.0, id)
            }
        }
    }

    /// A kept page-AI result from `main` or the session's loose library.
    pub fn saved<'a>(&'a self, main: &'a Library, id: AiPageId) -> Option<&'a AiPage> {
        main.ai_page(id).or_else(|| self.loose.0.ai_page(id))
    }

    pub fn lib<'a>(
        &'a self,
        main: &'a Library,
        book: Option<BookId>,
        name: &str,
    ) -> (&'a Library, BookId) {
        match book.filter(|b| main.book(*b).is_some()) {
            Some(id) => (main, id),
            None => (&self.loose.0, loose_id(name)),
        }
    }

    /// Writes the page's text; the same ask under way just shows.
    pub fn begin(
        &mut self,
        who: &Who<'_>,
        (page, last): (usize, usize),
        task: AiTask,
        (source, span): (Source, PageSpan),
        redo_of: Option<AiPageId>,
        now: f64,
    ) -> Result<u64, &'static str> {
        let key = (who.pane, who.layout, page);
        let same = |r: &&Run| r.layout == who.layout && r.task == task && r.span == span;
        let live = |r: &&Run| r.failed.is_none() && !r.requeue;
        if let Some(id) = self.runs.iter().find(same).filter(live).map(|r| r.id) {
            self.show_on(key, Slot::Run(id), now);
            return Ok(id);
        }
        self.runs
            .retain(|r| !(r.layout == who.layout && r.task == task && r.span == span));
        let pages = last.saturating_sub(page) + 1;
        let handle = match (self.canned, self.held) {
            (_, true) => Handle::Held(Some(canned(&source))),
            (true, false) => Handle::Canned(Some(canned(&source))),
            (false, false) => who.ai.start(&task, &source, pages)?,
        };
        let preset = who.settings.page_preset(page_task(&task));
        let by = preset.map_or_else(String::new, |p| match p.name.trim() {
            "" => p.model.clone(),
            name => format!("{name} · {}", p.model),
        });
        self.next_run += 1;
        let id = self.next_run;
        self.runs.push(Run {
            id,
            layout: who.layout,
            page,
            last: last.max(page),
            book: who.book,
            name: who.name.to_owned(),
            task,
            span,
            source: Arc::new(source),
            by,
            handle: Some(handle),
            shown: None,
            latest: None,
            shown_at: 0.0,
            generation: 0,
            failed: None,
            redo_of,
            requeue: false,
        });
        self.show_on(key, Slot::Run(id), now);
        Ok(id)
    }

    pub fn redo(
        &mut self,
        asked: Asked<'_>,
        task: AiTask,
        redo_of: Option<AiPageId>,
        now: f64,
    ) -> Option<u64> {
        let view = asked.documents.ai_view(asked.pane)?;
        let page = view.document.to_base(asked.page);
        let found = match source_of(&view, asked.documents, page) {
            Ok(found) => found,
            Err("loading") => {
                self.notes
                    .push("This page's text is still loading. Try again in a moment.".to_owned());
                return None;
            }
            Err(_) => {
                self.notes
                    .push("This page has no text to work on.".to_owned());
                return None;
            }
        };
        if found.0.is_blank() {
            let scan = matches!(*view.document, Document::Pdf(_));
            let why = match scan {
                true => "This page has no text to work on. It may be a scanned image.",
                false => "This page has no text to work on.",
            };
            self.notes.push(why.to_owned());
            return None;
        }
        let who = Who {
            pane: asked.pane,
            layout: view.layout,
            book: asked.book,
            name: asked.name,
            ai: asked.ai,
            settings: asked.settings,
        };
        match self.begin(&who, (page, page), task, found, redo_of, now) {
            Ok(id) => Some(id),
            Err(why) => {
                self.notes.push(why.to_owned());
                None
            }
        }
    }

    /// Shows `slot` on own page `key`; others of the same text give way.
    pub(crate) fn show_on(&mut self, key: Key, slot: Slot, now: f64) {
        // Only one result of any text is laid in, or covers a PDF's page.
        if let Some(mine) = self.covering(slot) {
            let (pane, layout, _) = key;
            let overlaps: Vec<Key> = (self.showing.iter())
                .filter(|((p, l, _), _)| *p == pane && *l == layout)
                .filter(|(_, s)| self.covering(s.slot).is_some_and(|c| c.meets(mine)))
                .map(|(k, _)| *k)
                .collect();
            for k in overlaps {
                self.showing.remove(&k);
            }
        }
        self.showing.insert(key, Showing { slot, since: now });
    }

    /// What `slot` covers; a book's run lays nothing in, so none.
    fn covering(&self, slot: Slot) -> Option<Cover> {
        match slot {
            Slot::Saved(id) => (self.spans.get(&id).map(|(a, b)| Cover::Text(*a, *b)))
                .or_else(|| self.covers.get(&id).map(|(a, b)| Cover::Pages(*a, *b))),
            Slot::Run(id) => match self.runs.iter().find(|r| r.id == id)?.span {
                PageSpan::Page { page } => Some(Cover::Pages(page, page)),
                PageSpan::Pages { from, to } => Some(Cover::Pages(from, to)),
                PageSpan::Blocks { .. } => None,
            },
        }
    }
}
