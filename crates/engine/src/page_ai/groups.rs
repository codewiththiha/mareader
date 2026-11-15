//! Picked pages asked in groups, as many at once as the model allows.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use mareader_library::{AiPageId, AiTask, BookId, PageSpan};
use mareader_session::Document;
use mareader_settings::{AiSettings, PageTask, Provider};
use mareader_workspace::PaneId;

use super::Slot;
use super::extract::{self, Source};
use super::follow::doing;
use super::ledger::Ledger;
use super::read;
use super::state::{Around, Asked, Who};
use super::view::{Pill, Ring, group_span, page_task};

/// A group's text, if any, the group, and the span it covers.
pub type Read = (Option<Source>, Group, PageSpan);

/// Times a group the model was too busy for is asked again.
pub const TRIES: u8 = 3;

/// Own pages `from` to `to`, both in, asked as one.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub task: AiTask,
    pub from: usize,
    pub to: usize,
    pub redo_of: Option<AiPageId>,
    pub tries: u8,
}

/// A pane's groups: waiting, being read, being written.
pub struct Batch {
    /// The layout the pages are counted in: a book's own.
    pub layout: u64,
    pub book: Option<BookId>,
    pub name: String,
    pub waiting: VecDeque<Group>,
    pub reading: Vec<(Receiver<Option<Source>>, Group, PageSpan)>,
    pub runs: Vec<(u64, Group)>,
    pub total: usize,
    pub finished: usize,
    pub blank: usize,
}

impl Batch {
    pub fn waiting_any(&self) -> bool {
        !self.waiting.is_empty()
    }

    pub fn flying(&self) -> usize {
        self.runs.len() + self.reading.len()
    }

    pub fn has(&self, g: &Group) -> bool {
        let same = |o: &Group| o.task == g.task && o.from == g.from && o.to == g.to;
        self.waiting.iter().any(same)
            || self.runs.iter().any(|(_, o)| same(o))
            || self.reading.iter().any(|(_, o, _)| same(o))
    }

    /// The task now being done, for the pill.
    pub fn task(&self) -> Option<&AiTask> {
        let flying = self.runs.iter().map(|(_, g)| g);
        let reading = self.reading.iter().map(|(_, g, _)| g);
        let mut all = flying.chain(reading).chain(self.waiting.iter());
        all.next().map(|g| &g.task)
    }
}

/// Own pages in order, cut into runs of neighbours of at most `size`.
pub fn groups(pages: &[usize], size: usize) -> Vec<(usize, usize)> {
    let mut pages = pages.to_vec();
    pages.sort_unstable();
    pages.dedup();
    let mut out: Vec<(usize, usize)> = Vec::new();
    for page in pages {
        match out.last_mut() {
            Some((from, to)) if *to + 1 == page && page - *from < size.max(1) => *to = page,
            _ => out.push((page, page)),
        }
    }
    out
}

/// Pages per request: the setting, fewer for a model with little context.
pub fn size_for(settings: &AiSettings, task: &AiTask) -> usize {
    let size = usize::from(settings.tasks.pages.per_request.max(1));
    let preset = settings.page_preset(page_task(task));
    // Apple's model reads about 4,000 tokens in all, answer included.
    match preset.map(|p| p.provider) {
        Some(Provider::Apple) if page_task(task) == PageTask::Summarize => size.min(2),
        Some(Provider::Apple) => 1,
        _ => size,
    }
}

/// What a group covers in the book: its span, or none without text.
pub fn span_of(document: &Document, (from, to): (usize, usize)) -> Option<PageSpan> {
    match document {
        Document::Epub { .. } => {
            let [from, to] = group_span(document, (from, to))?;
            Some(PageSpan::Blocks { from, to })
        }
        Document::Pdf(_) if from == to => Some(PageSpan::Page { page: from }),
        Document::Pdf(_) => Some(PageSpan::Pages { from, to }),
    }
}

/// Groups written at once: the model's room, else its own setting.
pub fn room_for(a: &Around<'_>, task: &AiTask) -> usize {
    let Some(preset) = a.settings.page_preset(page_task(task)) else {
        return 1;
    };
    let room = a.ai.capacity(preset).unwrap_or(u32::from(preset.at_once));
    room.max(1) as usize
}

impl Ledger {
    /// Queues the picked pages of `asked`'s pane, in groups, for `task`.
    pub fn run_pages(&mut self, asked: Asked<'_>, pages: &[usize], task: AiTask) {
        let Some(view) = asked.documents.ai_view(asked.pane) else {
            return;
        };
        let own: Vec<usize> = pages.iter().map(|p| view.document.to_base(*p)).collect();
        let size = size_for(asked.settings, &task);
        let groups = groups(&own, size).into_iter().map(|(from, to)| Group {
            task: task.clone(),
            from,
            to,
            redo_of: None,
            tries: 0,
        });
        self.queue(asked, view.layout, groups.collect());
    }

    /// Asks a kept or failed group's pages again, replacing its result.
    pub fn redo_group(
        &mut self,
        asked: Asked<'_>,
        layout: u64,
        (task, from, to): (AiTask, usize, usize),
        redo_of: Option<AiPageId>,
    ) {
        let group = Group {
            task,
            from,
            to,
            redo_of,
            tries: 0,
        };
        self.queue(asked, layout, vec![group]);
    }

    fn queue(&mut self, asked: Asked<'_>, layout: u64, groups: Vec<Group>) {
        let fresh = || Batch {
            layout,
            book: asked.book,
            name: asked.name.to_owned(),
            waiting: VecDeque::new(),
            reading: Vec::new(),
            runs: Vec::new(),
            total: 0,
            finished: 0,
            blank: 0,
        };
        let batch = self.batches.entry(asked.pane).or_insert_with(fresh);
        // Another book or text size: the old pages mean nothing now.
        if batch.layout != layout {
            *batch = fresh();
        }
        for g in groups {
            if !batch.has(&g) {
                batch.waiting.push_back(g);
                batch.total += 1;
            }
        }
    }

    /// Each pane's groups: taken in, asked again, or started.
    pub fn drive(&mut self, a: &Around<'_>, now: f64) {
        let panes: Vec<PaneId> = self.batches.keys().copied().collect();
        for pane in panes {
            let Some(mut b) = self.batches.remove(&pane) else {
                continue;
            };
            if self.step_batch(pane, &mut b, a, now) {
                self.batches.insert(pane, b);
            }
        }
    }

    /// One frame of `b`; false once it is done.
    fn step_batch(&mut self, pane: PaneId, b: &mut Batch, a: &Around<'_>, now: f64) -> bool {
        let Some(view) = a.documents.ai_view(pane) else {
            return false;
        };
        if view.layout != b.layout {
            if !b.waiting.is_empty() {
                let why = "The text size changed, so the pages left weren't asked.";
                self.notes.push(why.to_owned());
            }
            return false;
        }
        let name = b.name.clone();
        let who = Who {
            pane,
            layout: b.layout,
            book: b.book,
            name: &name,
            ai: a.ai,
            settings: a.settings,
        };
        // Pages read off the frame start writing.
        let mut read = Vec::new();
        b.reading.retain(|(rx, g, span)| match rx.try_recv() {
            Ok(found) => {
                read.push((found, g.clone(), span.clone()));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                read.push((None, g.clone(), span.clone()));
                false
            }
        });
        for r in read {
            self.write(b, &who, r, now);
        }
        // Runs done, failed, or turned away for a busy model.
        let mut stop = false;
        let mut left = Vec::new();
        for (id, mut g) in std::mem::take(&mut b.runs) {
            let Some(run) = self.runs.iter().find(|r| r.id == id) else {
                b.finished += 1;
                continue;
            };
            if run.requeue {
                self.runs.retain(|r| r.id != id);
                self.showing.retain(|_, s| s.slot != Slot::Run(id));
                g.tries += 1;
                match g.tries < TRIES {
                    true => b.waiting.push_front(g),
                    false => {
                        let why = "The model stayed busy, so some pages weren't done.";
                        self.notes.push(why.to_owned());
                        b.finished += 1;
                    }
                }
            } else if run.failed.is_some() {
                b.finished += 1;
                stop = true;
            } else {
                left.push((id, g));
            }
        }
        b.runs = left;
        if stop {
            b.finished += b.waiting.len();
            b.waiting.clear();
        }
        let room = b.task().map_or(1, |task| room_for(a, task));
        while b.flying() < room {
            let Some(g) = b.waiting.pop_front() else {
                break;
            };
            self.launch(b, &who, g, &view.document, a, now);
        }
        if b.waiting.is_empty() && b.flying() == 0 {
            if b.blank > 0 && b.blank == b.total {
                let why = "These pages have no text to work on.";
                self.notes.push(why.to_owned());
            }
            return false;
        }
        true
    }

    /// Starts group `g`: its kept result shows, else it is read.
    fn launch(
        &mut self,
        b: &mut Batch,
        who: &Who<'_>,
        g: Group,
        document: &Arc<Document>,
        a: &Around<'_>,
        now: f64,
    ) {
        let Some(span) = span_of(document, (g.from, g.to)) else {
            b.blank += 1;
            b.finished += 1;
            return;
        };
        if g.redo_of.is_none() {
            let (lib, book) = self.lib(a.main, who.book, who.name);
            let kept = (lib.ai_pages(book).iter())
                .find(|p| p.task == g.task && p.span == span)
                .map(|p| p.id);
            if let Some(id) = kept {
                self.show_on((who.pane, who.layout, g.from), Slot::Saved(id), now);
                b.finished += 1;
                return;
            }
        }
        match (&**document, &span) {
            (Document::Epub { book, .. }, PageSpan::Blocks { from, to }) => {
                let source = extract::book(book.blocks(), [*from, *to]);
                self.write(b, who, (Some(source), g, span), now);
            }
            _ => {
                let wake = self.repaint.waker();
                let rx = read::group(wake, Arc::clone(document), (g.from, g.to), self.inline);
                b.reading.push((rx, g, span));
            }
        }
    }

    /// Writes a read group, or counts it as without text.
    fn write(&mut self, b: &mut Batch, who: &Who<'_>, read: Read, now: f64) {
        let (found, g, span) = read;
        let Some(source) = found.filter(|s| !s.is_blank()) else {
            b.blank += 1;
            b.finished += 1;
            return;
        };
        let pages = (g.from, g.to);
        let task = g.task.clone();
        match self.begin(who, pages, task, (source, span), g.redo_of, now) {
            Ok(id) => b.runs.push((id, g)),
            Err(why) => {
                self.notes.push(why.to_owned());
                b.finished += b.waiting.len() + 1;
                b.waiting.clear();
            }
        }
    }

    /// Stops what the pane's pill shows: its continuous run, else groups.
    pub fn stop_pill(&mut self, pane: PaneId) {
        match self.following(pane).is_some() {
            true => self.stop_follow(pane),
            false => self.stop_groups(pane),
        }
    }

    /// Whether `pane` has groups under way, for the menu.
    pub fn grouping(&self, pane: PaneId) -> bool {
        self.batches.contains_key(&pane)
    }

    /// Drops the pane's waiting groups and stops those being written.
    pub fn stop_groups(&mut self, pane: PaneId) {
        let Some(b) = self.batches.remove(&pane) else {
            return;
        };
        let ids: Vec<u64> = b.runs.iter().map(|(id, _)| *id).collect();
        self.runs.retain(|r| !ids.contains(&r.id));
        self.showing
            .retain(|_, s| !matches!(s.slot, Slot::Run(id) if ids.contains(&id)));
    }

    /// The pane's groups as its pill shows them.
    pub fn group_pill(&self, pane: PaneId) -> Option<Pill> {
        let b = self.batches.get(&pane)?;
        let (title, _) = doing(b.task()?);
        let total = b.total.max(1);
        let tip = match (b.finished, b.waiting.len()) {
            (done, 0) => format!("{done} of {total} groups of pages done"),
            (done, waiting) => {
                format!("{done} of {total} groups of pages done, {waiting} waiting for the model")
            }
        };
        Some(Pill {
            text: format!("{title} · {}/{total}", b.finished),
            tip,
            ring: Ring::Fill(b.finished as f32 / total as f32),
            ask: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::groups;

    #[test]
    fn picks_split_into_neighbours_at_most_the_size() {
        let pages: Vec<usize> = (0..11).collect();
        assert_eq!(groups(&pages, 5), [(0, 4), (5, 9), (10, 10)]);
        assert_eq!(groups(&[7, 2, 3, 3, 9, 8, 4], 5), [(2, 4), (7, 9)]);
        assert_eq!(groups(&[1, 2, 3], 1), [(1, 1), (2, 2), (3, 3)]);
        assert!(groups(&[], 5).is_empty());
    }
}
