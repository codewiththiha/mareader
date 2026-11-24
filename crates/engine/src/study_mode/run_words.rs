//! Words mode on a run: each page's hard words counted from the first page
//! on, and the batches planned by them.
//!
//! The first plan waits for 60 pages (`counts::WINDOW`); the pages past
//! the count stand as pages-mode batches until it reaches them. Each round
//! counted plans again from the first batch not in use: one passed, at the
//! gate with a deck, or with a deck made ahead keeps its pages. Without the
//! word data, words mode plans by pages.

use mareader_session::Document;
use mareader_study::mode::{Batch, Size, Unit, plan_from, plan_words};

use super::{Ahead, Run};
use crate::study_mode::counts::{Counts, page_words};
use crate::study_mode::known::Known;
use crate::study_mode::sightings::Read;
use crate::word_list::Levels;

/// What words mode read and counted on the current layout.
#[derive(Default)]
pub struct Tally {
    /// Each page's words as read, measured with the sizes.
    reads: Vec<Vec<Read>>,
    counts: Option<Counts>,
    /// The page counting starts at: the run's first.
    from: usize,
    /// No word data: words mode plans by pages.
    off: bool,
}

impl Tally {
    /// A layout's words, to be counted afresh.
    pub fn measured(reads: Vec<Vec<Read>>) -> Self {
        Self {
            reads,
            ..Self::default()
        }
    }
}

impl Run {
    /// Words mode has pages left to count.
    pub fn wants_count(&self, size: Size) -> bool {
        let all = |c: &Counts| c.done() >= self.tally.reads.len();
        let done = self.tally.counts.as_ref().is_some_and(all);
        size.unit == Unit::Words
            && self.layout.is_some()
            && self.measuring.is_none()
            && !self.tally.off
            && !done
    }

    /// Counting is under way, so the session keeps ticking.
    pub fn counting(&self) -> bool {
        let left = |c: &Counts| c.done() < self.tally.reads.len();
        !self.tally.off && self.tally.counts.as_ref().is_some_and(left)
    }

    /// Counts a round of pages at `level`; true when something moved.
    pub fn count(&mut self, size: Size, levels: &mut dyn Levels, known: &Known, level: u8) -> bool {
        if !self.wants_count(size) || levels.coming() {
            return false;
        }
        if !levels.ready() {
            self.tally.off = true;
            self.stale = true;
            return true;
        }
        let from = self.tally.from;
        let fresh = |c: &Counts| (c.from, c.level) == (from, level);
        if !self.tally.counts.as_ref().is_some_and(fresh) {
            self.tally.counts = Some(Counts::new(from, level));
        }
        let Some(counts) = &mut self.tally.counts else {
            return false;
        };
        if !counts.step(&self.tally.reads, levels, known) {
            return false;
        }
        if self.planned() && self.size == Some(size) {
            self.replan_tail(size);
        }
        true
    }

    /// Words mode waits to plan from `start` until a window is counted.
    pub(super) fn words_wait(&mut self, size: Size, start: usize) -> bool {
        if size.unit != Unit::Words {
            self.tally.counts = None;
            return false;
        }
        self.tally.from = start;
        let total = self.sizes.len();
        let ready = |c: &Counts| c.from == start && c.enough(total);
        !self.tally.off && !self.tally.counts.as_ref().is_some_and(ready)
    }

    /// The batches from page `from` on: by words as far as counted, then
    /// by pages.
    pub(super) fn layout_batches(&self, size: Size, from: usize) -> Vec<Batch> {
        let pages = Size {
            unit: Unit::Pages,
            ..size
        };
        let on = size.unit == Unit::Words && !self.tally.off;
        let counts = self.tally.counts.as_ref().filter(|_| on);
        let mut out = Vec::new();
        if let Some(counts) = counts.filter(|c| from < c.done()) {
            out = plan_words(counts.pages(), &self.sizes, size.words, from);
        }
        let rest = out.last().map_or(from, |b| b.to + 1);
        out.extend(plan_from(&self.sizes, pages, rest));
        numbered(out)
    }

    /// Plans again past the batches in use, as more pages are counted.
    fn replan_tail(&mut self, size: Size) {
        let ahead = self.ahead.iter().map(Ahead::batch);
        let in_use = self.deck.batch().into_iter().chain(ahead);
        let used = in_use.map(|b| b.to + 1).max().unwrap_or(0);
        let freeze = used.max(self.unlocked).max(self.start);
        let kept = self.batches.iter().filter(|b| b.to < freeze);
        let mut out: Vec<Batch> = kept.cloned().collect();
        out.extend(self.layout_batches(size, freeze));
        self.batches = numbered(out);
    }
}

/// Batches counted from 0 in order.
fn numbered(mut batches: Vec<Batch>) -> Vec<Batch> {
    for (i, b) in batches.iter_mut().enumerate() {
        b.index = i;
    }
    batches
}

/// Each page's character count and words, as the reader's text holds them.
pub(super) fn measure(document: &Document) -> (Vec<usize>, Tally) {
    let count = document.page_count();
    let mut sizes = Vec::with_capacity(count);
    let mut reads = Vec::with_capacity(count);
    for page in 0..count {
        let text = match document.text_layer(page) {
            Ok(layer) => crate::study::words::read(&layer).0,
            Err(_) => Vec::new(),
        };
        sizes.push(text.len());
        reads.push(page_words(&text.iter().collect::<String>()));
    }
    (sizes, Tally::measured(reads))
}
