//! A pane's continuous run: the pages ahead, asked as the reader reads.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Duration;

use mareader_library::{AiTask, BookId, Library};
use mareader_session::Document;
use mareader_settings::{AiSettings, language_name};
use mareader_workspace::PaneId;

use super::Slot;
use super::ledger::Ledger;
use super::read::{self, Read};
use super::state::{Around, Asked, Showing, Who};
use super::view::{Models, Pill, Ring, page_task, source_of};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Going,
    /// The reader went far from the run's pages.
    Away,
    /// Still, far from them: the pill asks whether to carry on.
    Asking,
}

/// A pane's continuous run.
pub struct Follow {
    /// The layout its pages are counted in: a book's own.
    pub layout: u64,
    pub task: AiTask,
    pub book: Option<BookId>,
    pub name: String,
    /// Pages asked: from `from` up to, not with, `next`.
    pub from: usize,
    pub next: usize,
    /// The page count it was asked in; a new layout starts afresh.
    pub pages: usize,
    /// The page read, and since when its result showed.
    pub dwell: Option<(usize, f64)>,
    pub dwell_secs: f64,
    /// The last page whose stay asked more.
    pub fired: Option<usize>,
    /// The page last seen, and since when.
    pub seen: (usize, f64),
    pub state: State,
    /// Runs it started, and pages found without text.
    pub runs: Vec<u64>,
    pub blank: HashSet<usize>,
    pub reading: Vec<Receiver<Read>>,
    /// Pages being read off the frame.
    pub pending: usize,
}

/// The pill's words for a task: what it does, and its verb.
pub fn doing(task: &AiTask) -> (String, &'static str) {
    match task {
        AiTask::Translate { language } => {
            let name = language_name(language).unwrap_or(language);
            (format!("Translating to {name}"), "translating")
        }
        AiTask::Simplify => ("Simplifying pages".to_owned(), "simplifying"),
        AiTask::Summary | AiTask::Points => ("Summarizing pages".to_owned(), "summarizing"),
    }
}

/// Pages asked at once: the setting, or fewer if the model has less room.
pub fn allowance(ai: &dyn Models, settings: &AiSettings, task: &AiTask) -> usize {
    let cap = usize::from(settings.tasks.pages.at_once.max(1));
    let preset = settings.page_preset(page_task(task));
    let room = preset.and_then(|p| ai.capacity(p));
    room.map_or(cap, |r| cap.min(r.max(1) as usize))
}

/// Seconds the page keeps still before the run acts on it.
const STILL: f64 = 1.2;
/// Pages past the run's ends that still count as reading it.
const SLACK: usize = 2;

impl Ledger {
    /// Starts the pane's continuous run here; `fresh` rewrites a kept page.
    pub fn follow(
        &mut self,
        asked: Asked<'_>,
        main: &Library,
        task: AiTask,
        fresh: bool,
        now: f64,
    ) {
        let Some(view) = asked.documents.ai_view(asked.pane) else {
            return;
        };
        let (pane, page) = (asked.pane, view.document.to_base(asked.page));
        self.stop_follow(pane);
        let mut f = Follow {
            layout: view.layout,
            task: task.clone(),
            book: asked.book,
            name: asked.name.to_owned(),
            from: page,
            next: page + 1,
            pages: view.document.base_count(),
            dwell: None,
            dwell_secs: f64::from(asked.settings.tasks.pages.dwell.max(1)),
            fired: None,
            seen: (page, now),
            state: State::Going,
            runs: Vec::new(),
            blank: HashSet::new(),
            reading: Vec::new(),
            pending: 0,
        };
        match source_of(&view, asked.documents, page) {
            Ok(found) if found.0.is_blank() => {
                f.blank.insert(page);
            }
            Ok(found) if !fresh => {
                let around = Around {
                    main,
                    documents: asked.documents,
                    ai: asked.ai,
                    settings: asked.settings,
                };
                self.take(&mut f, pane, (page, Some(found)), &around, now);
            }
            Err("blank") => {
                f.blank.insert(page);
            }
            // A fresh page, or one still loading: the page's own run says.
            _ => match self.redo(asked, task, None, now) {
                Some(id) => f.runs.push(id),
                None => return,
            },
        }
        self.follows.insert(pane, f);
    }

    /// Ends the pane's continuous run; pages ahead no one saw are let go.
    pub fn stop_follow(&mut self, pane: PaneId) {
        if let Some(f) = self.follows.remove(&pane) {
            self.let_go(&f);
        }
    }

    /// Whether `pane` has a continuous run, for the menu.
    pub fn following(&self, pane: PaneId) -> Option<&AiTask> {
        self.follows.get(&pane).map(|f| &f.task)
    }

    fn let_go(&mut self, f: &Follow) {
        let read = f.seen.0;
        let ahead: Vec<u64> = (self.runs.iter())
            .filter(|r| f.runs.contains(&r.id) && r.page > read && r.handle.is_some())
            .map(|r| r.id)
            .collect();
        self.runs.retain(|r| !ahead.contains(&r.id));
        self.showing
            .retain(|_, s| !matches!(s.slot, Slot::Run(id) if ahead.contains(&id)));
    }

    /// Carries the pane's run on from the page it asked about.
    pub fn resume(&mut self, asked: Asked<'_>, main: &Library, now: f64) {
        let Some(task) = self.following(asked.pane).cloned() else {
            return;
        };
        self.follow(asked, main, task, false, now);
    }

    /// Moves each continuous run along, and each pane's picked groups.
    pub fn tick(&mut self, a: &Around<'_>, now: f64) {
        self.drive(a, now);
        let panes: Vec<PaneId> = self.follows.keys().copied().collect();
        for pane in panes {
            let Some(mut f) = self.follows.remove(&pane) else {
                continue;
            };
            match self.step(pane, &mut f, a, now) {
                true => {
                    self.follows.insert(pane, f);
                }
                false => self.let_go(&f),
            }
        }
    }

    /// One frame of `f`; false once it should end.
    fn step(&mut self, pane: PaneId, f: &mut Follow, a: &Around<'_>, now: f64) -> bool {
        let Some(view) = a.documents.ai_view(pane).filter(|v| v.layout == f.layout) else {
            return false;
        };
        // A run whose page failed ends; that page's bubble offers a retry.
        let failed = (self.runs.iter()).any(|r| f.runs.contains(&r.id) && r.failed.is_some());
        if failed {
            return false;
        }
        f.runs.retain(|id| self.runs.iter().any(|r| r.id == *id));
        let cur = view.document.to_base(view.current);
        let count = view.document.base_count();
        if count != f.pages {
            // A new layout moved every page: the run starts here.
            (f.pages, f.from, f.next) = (count, cur, cur);
            (f.dwell, f.fired) = (None, None);
            f.blank.clear();
        }
        if f.seen.0 != cur {
            f.seen = (cur, now);
        }
        f.dwell_secs = f64::from(a.settings.tasks.pages.dwell.max(1));
        self.collect(f, pane, a, now);
        let n = allowance(a.ai, a.settings, &f.task);
        let gap = n + SLACK;
        let inside = cur + gap >= f.from && cur < f.next + gap;
        let still = now - f.seen.1 >= STILL;
        f.state = match (f.state, inside) {
            (_, true) => State::Going,
            (State::Going, false) => State::Away,
            (_, false) if still => State::Asking,
            (_, false) => State::Away,
        };
        if f.state == State::Away {
            self.repaint.after(Duration::from_secs_f64(0.3));
        }
        if f.state != State::Going {
            f.dwell = None;
            return true;
        }
        let shown = f.blank.contains(&cur) || self.ready_on(pane, f.layout, cur);
        if shown {
            let since = match f.dwell {
                Some((p, since)) if p == cur => since,
                _ => {
                    f.dwell = Some((cur, now));
                    now
                }
            };
            if now - since < f.dwell_secs {
                self.repaint.after(Duration::from_secs_f64(0.25));
            } else if f.fired != Some(cur) {
                f.fired = Some(cur);
                self.top_up(f, (pane, cur, n), a, now);
            }
            return true;
        }
        f.dwell = None;
        if cur >= f.next && cur < count {
            // Read past what was asked: this page, once it keeps still.
            if still && f.pending == 0 {
                f.next = cur + 1;
                self.cover(f, pane, &[cur], a, now);
            } else {
                self.repaint.after(Duration::from_secs_f64(0.3));
            }
        }
        true
    }

    /// Asks pages ahead of `cur` until `n` are asked or under way.
    fn top_up(
        &mut self,
        f: &mut Follow,
        (pane, cur, n): (PaneId, usize, usize),
        a: &Around<'_>,
        now: f64,
    ) {
        let ahead = f.next.saturating_sub(cur + 1);
        let flying = self.flying(f);
        let want = n.saturating_sub(ahead).min(n.saturating_sub(flying));
        let start = f.next.max(cur + 1);
        let end = (start + want).min(f.pages);
        if end <= start {
            return;
        }
        f.next = end;
        let pages: Vec<usize> = (start..end).collect();
        self.cover(f, pane, &pages, a, now);
    }

    /// Pages of `f` being read or written now.
    fn flying(&self, f: &Follow) -> usize {
        let writing = (self.runs.iter())
            .filter(|r| f.runs.contains(&r.id) && r.handle.is_some())
            .count();
        writing + f.pending
    }

    fn cover(&mut self, f: &mut Follow, pane: PaneId, pages: &[usize], a: &Around<'_>, now: f64) {
        let Some(view) = a.documents.ai_view(pane) else {
            return;
        };
        match &*view.document {
            Document::Epub { .. } => {
                for &page in pages {
                    let found = source_of(&view, a.documents, page).ok();
                    self.take(f, pane, (page, found), a, now);
                }
            }
            Document::Pdf(_) => {
                f.pending += pages.len();
                let document = Arc::clone(&view.document);
                let wake = self.repaint.waker();
                f.reading
                    .push(read::pages(wake, document, pages.to_vec(), self.inline));
            }
        }
    }

    /// Pages read off the frame, taken in.
    fn collect(&mut self, f: &mut Follow, pane: PaneId, a: &Around<'_>, now: f64) {
        let mut got = Vec::new();
        f.reading.retain(|rx| {
            loop {
                match rx.try_recv() {
                    Ok(read) => got.push(read),
                    Err(TryRecvError::Empty) => break true,
                    Err(TryRecvError::Disconnected) => break false,
                }
            }
        });
        f.pending = match f.reading.is_empty() {
            true => 0,
            false => f.pending.saturating_sub(got.len()),
        };
        for read in got {
            self.take(f, pane, read, a, now);
        }
    }

    /// A page to cover: its kept result shows, else it is written.
    fn take(&mut self, f: &mut Follow, pane: PaneId, read: Read, a: &Around<'_>, now: f64) {
        let (page, found) = read;
        let Some((source, span)) = found.filter(|(s, _)| !s.is_blank()) else {
            f.blank.insert(page);
            return;
        };
        let (lib, book) = self.lib(a.main, f.book, &f.name);
        let kept = (lib.ai_pages(book).iter())
            .find(|p| p.task == f.task && p.span == span)
            .map(|p| p.id);
        let key = (pane, f.layout, page);
        if let Some(id) = kept {
            let slot = Slot::Saved(id);
            self.showing
                .entry(key)
                .or_insert(Showing { slot, since: now });
            return;
        }
        let who = Who {
            pane,
            layout: f.layout,
            book: f.book,
            name: &f.name,
            ai: a.ai,
            settings: a.settings,
        };
        let task = f.task.clone();
        if let Ok(id) = self.begin(&who, (page, page), task, (source, span), None, now) {
            f.runs.push(id);
        }
    }

    /// Whether a result with words in it covers the page.
    fn ready_on(&self, pane: PaneId, layout: u64, page: usize) -> bool {
        match self.showing.get(&(pane, layout, page)).map(|s| s.slot) {
            Some(Slot::Saved(_)) => true,
            Some(Slot::Run(id)) => self.runs.iter().any(|r| r.id == id && r.shown.is_some()),
            None => false,
        }
    }

    /// The pane's run as its pill shows it.
    pub fn pill(&self, pane: PaneId, now: f64) -> Option<Pill> {
        let f = self.follows.get(&pane)?;
        let (title, verb) = doing(&f.task);
        let cur = f.seen.0;
        let pill = |text: String, tip: String, ring: Ring| Pill {
            text,
            tip,
            ring,
            ask: false,
        };
        Some(match f.state {
            State::Away => pill(
                format!("{title} · paused"),
                "You went far from the pages being worked on.".to_owned(),
                Ring::Paused,
            ),
            State::Asking => Pill {
                text: format!("Continue {verb} here?"),
                tip: format!("Carry on {verb} from this page, or stop."),
                ring: Ring::Paused,
                ask: true,
            },
            State::Going => {
                let flying = self.flying(f);
                let shown = f.blank.contains(&cur) || self.ready_on(pane, f.layout, cur);
                let waiting = f.dwell.filter(|(p, _)| *p == cur && f.fired != Some(cur));
                if !shown && cur >= f.from && cur < f.next {
                    pill(title, "Writing this page…".to_owned(), Ring::Spin)
                } else if !shown && cur < f.from {
                    let tip = "It works from where it started, onwards.".to_owned();
                    pill(title, tip, Ring::Fill(0.0))
                } else if !shown {
                    let tip = "Stay on this page to have it done too.".to_owned();
                    pill(title, tip, Ring::Fill(0.0))
                } else if let Some((_, since)) = waiting.filter(|_| f.next < f.pages) {
                    let left = (f.dwell_secs - (now - since)).max(0.0).ceil();
                    let tip = format!("In {left:.0} s on this page, the next pages are asked.");
                    let p = ((now - since) / f.dwell_secs) as f32;
                    pill(title, tip, Ring::Fill(p))
                } else if flying > 0 {
                    let tip = match flying {
                        1 => "Writing the next page…".to_owned(),
                        n => format!("Writing the next {n} pages…"),
                    };
                    pill(title, tip, Ring::Full)
                } else if f.next >= f.pages {
                    pill(title, "Done to the end.".to_owned(), Ring::Full)
                } else {
                    pill(title, "The next pages are ready.".to_owned(), Ring::Full)
                }
            }
        })
    }
}
