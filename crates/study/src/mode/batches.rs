//! The batch planner: a book's pages as study batches (decision D3).
//!
//! A batch is whole pages. Pages mode counts pages that carry text, 1 to
//! [`MAX_PAGES`]; light pages (a blank leaf, a lone heading) use no quota and
//! ride with a neighbour, and a short tail joins the batch before it while
//! that stays within [`MAX_PAGES`].
//!
//! Words mode counts the hard words each page holds and breaks the pages so
//! every batch's deck comes as near Words per deck as whole pages allow
//! ([`even`]). A word never moves to another batch's deck: studied later, it
//! would be read first.

use std::collections::HashSet;
use std::ops::Range;

use serde::{Deserialize, Serialize};

use super::chunks;

pub const MAX_PAGES: usize = 10;
/// A page with fewer characters is light.
pub const LIGHT_CHARS: usize = 140;
/// Pages one batch may span in words mode, however few words they hold.
pub const MAX_WORD_PAGES: usize = 20;

/// What a batch's size is counted in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    /// Hard words; the old characters mode reads as this.
    #[default]
    #[serde(alias = "chars")]
    Words,
    Pages,
}

/// How big a batch should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub unit: Unit,
    pub pages: usize,
    /// Words per deck, the target in words mode.
    pub words: usize,
}

/// Pages `from` to `to`, both included, counted from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub index: usize,
    pub from: usize,
    pub to: usize,
    pub chars: usize,
    /// Distinct hard words, counted in words mode.
    pub words: usize,
    /// Light pages riding along.
    pub light: usize,
    /// A short tail was merged into it.
    pub merged: bool,
}

impl Batch {
    pub fn pages(&self) -> usize {
        self.to.saturating_sub(self.from).saturating_add(1)
    }

    pub fn holds(&self, page: usize) -> bool {
        (self.from..=self.to).contains(&page)
    }

    /// Its pages as a reader counts them: `page 4` or `pages 4–6`.
    pub fn label(&self) -> String {
        let (from, to) = (self.from.saturating_add(1), self.to.saturating_add(1));
        match from == to {
            true => format!("page {from}"),
            false => format!("pages {from}–{to}"),
        }
    }
}

/// The batches of pages with `sizes` characters each, in order, by page
/// count; words mode comes here while its words are not counted yet.
pub fn plan(sizes: &[usize], size: Size) -> Vec<Batch> {
    let quota = size.pages.clamp(1, MAX_PAGES);
    let mut out: Vec<Batch> = Vec::new();
    let mut i = 0;
    while i < sizes.len() {
        let (mut j, mut chars, mut heavy, mut light) = (i, 0usize, 0usize, 0usize);
        while j < sizes.len() && j - i < MAX_PAGES {
            let s = sizes.get(j).copied().unwrap_or(0);
            let is_light = s < LIGHT_CHARS;
            if j > i && !is_light && heavy >= quota {
                break;
            }
            chars = chars.saturating_add(s);
            if is_light {
                light += 1;
            } else {
                heavy += 1;
            }
            j += 1;
        }
        out.push(Batch {
            index: out.len(),
            from: i,
            to: j.saturating_sub(1),
            chars,
            words: 0,
            light,
            merged: false,
        });
        i = j.max(i + 1);
    }
    merge_tail(&mut out);
    out
}

/// Page breaks whose batches each hold near `target` distinct words, as
/// line breaking spaces words in a paragraph (after Knuth and Plass).
///
/// `words` holds each page's hard words. The breaks keep the sum of
/// (words − target)² least over all the pages, not just the next batch,
/// so no batch is left with a few words at the end. A batch of several
/// pages stays one deck ([`chunks`] leaves it whole, under 1.5× the
/// target); a page with more stands alone, as a page is never split.
pub fn even(words: &[Vec<u32>], target: usize) -> Vec<Range<usize>> {
    let target = target.max(1);
    // Per end page: least cost, fewest batches, last batch's start.
    let mut best = vec![(u64::MAX, 0usize, 0usize); words.len().saturating_add(1)];
    if let Some(first) = best.first_mut() {
        *first = (0, 0, 0);
    }
    for end in 1..best.len() {
        let mut seen: HashSet<u32> = HashSet::new();
        for from in (end.saturating_sub(MAX_WORD_PAGES)..end).rev() {
            seen.extend(words.get(from).into_iter().flatten().copied());
            let alone = from + 1 == end;
            if !alone && chunks(seen.len(), target).len() > 1 {
                break;
            }
            let (before, count, _) = best.get(from).copied().unwrap_or((u64::MAX, 0, 0));
            let gap = seen.len().abs_diff(target);
            let off = u64::try_from(gap).unwrap_or(u64::MAX);
            let cost = (before.saturating_add(off.saturating_mul(off)), count + 1);
            let slot = best.get_mut(end);
            if let Some(slot) = slot.filter(|s| cost < (s.0, s.1)) {
                *slot = (cost.0, cost.1, from);
            }
        }
    }
    let mut out = Vec::new();
    let mut end = words.len();
    while end > 0 {
        let from = best.get(end).map_or(0, |b| b.2);
        out.push(from..end);
        end = from;
    }
    out.reverse();
    out
}

/// The batches from page `start` on by their hard words, `words` and
/// `sizes` holding every page's.
pub fn plan_words(words: &[Vec<u32>], sizes: &[usize], target: usize, start: usize) -> Vec<Batch> {
    let rest = words.get(start..).unwrap_or_default();
    let batch = |(index, pages): (usize, Range<usize>)| {
        let (from, to) = (pages.start + start, pages.end + start);
        let pages = words.get(from..to).unwrap_or_default();
        let held: HashSet<u32> = pages.iter().flatten().copied().collect();
        Batch {
            index,
            from,
            to: to.saturating_sub(1),
            chars: sizes.get(from..to).unwrap_or_default().iter().sum(),
            words: held.len(),
            light: 0,
            merged: false,
        }
    };
    let breaks = even(rest, target).into_iter().enumerate();
    breaks.map(batch).collect()
}

/// A tail under 40% of the batch before it joins that batch.
fn merge_tail(out: &mut Vec<Batch>) {
    let n = out.len();
    let (Some(last), Some(prev)) = (
        n.checked_sub(1).and_then(|i| out.get(i)).cloned(),
        n.checked_sub(2).and_then(|i| out.get(i)),
    ) else {
        return;
    };
    let short = last.chars.saturating_mul(5) < prev.chars.saturating_mul(2);
    if !short || prev.pages() + last.pages() > MAX_PAGES {
        return;
    }
    out.pop();
    if let Some(prev) = out.last_mut() {
        prev.to = last.to;
        prev.chars = prev.chars.saturating_add(last.chars);
        prev.light += last.light;
        prev.merged = true;
    }
}

/// The first batch not yet unlocked: its words come before its pages.
pub fn gate(batches: &[Batch], unlocked: usize) -> Option<&Batch> {
    batches.iter().find(|b| b.from >= unlocked)
}

/// Progress after a re-typeset: rounded up to the end of its batch.
pub fn retypeset(unlocked: usize, batches: &[Batch], count: usize) -> usize {
    let Some(last) = unlocked.checked_sub(1) else {
        return 0;
    };
    batches
        .iter()
        .find(|b| b.holds(last))
        .map_or(count, |b| b.to + 1)
}

/// The batches from page `start` on; the pages before it are not studied.
pub fn plan_from(sizes: &[usize], size: Size, start: usize) -> Vec<Batch> {
    let rest = sizes.get(start..).unwrap_or_default();
    let mut out = plan(rest, size);
    for b in &mut out {
        b.from = b.from.saturating_add(start);
        b.to = b.to.saturating_add(start);
    }
    out
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::single_range_in_vec_init
)]
mod tests {
    use super::*;

    fn pages(n: usize) -> Size {
        Size {
            unit: Unit::Pages,
            pages: n,
            words: 12,
        }
    }

    fn spans(b: &[Batch]) -> Vec<(usize, usize)> {
        b.iter().map(|b| (b.from, b.to)).collect()
    }

    #[test]
    fn pages_mode_counts_pages_and_light_ones_ride_along() {
        assert_eq!(spans(&plan(&[400; 9], pages(3))), [(0, 2), (3, 5), (6, 8)]);
        let b = plan(&[400, 50, 400, 400, 400], pages(2));
        assert_eq!(spans(&b), [(0, 2), (3, 4)]);
        assert_eq!((b[0].light, b[0].chars), (1, 850));
    }

    #[test]
    fn a_short_tail_merges_only_within_the_cap() {
        let b = plan(&[400; 7], pages(3));
        assert_eq!(spans(&b), [(0, 2), (3, 6)]);
        assert!(b[1].merged);
        assert_eq!(spans(&plan(&[400; 11], pages(10))), [(0, 9), (10, 10)]);
    }

    /// Pages holding `counts` distinct words each, none shared.
    fn pages_of(counts: &[u32]) -> Vec<Vec<u32>> {
        let mut next = 0;
        let page = |&n: &u32| {
            next += n;
            (next - n..next).collect()
        };
        counts.iter().map(page).collect()
    }

    fn breaks(counts: &[u32], target: usize) -> Vec<Range<usize>> {
        even(&pages_of(counts), target)
    }

    #[test]
    fn words_mode_breaks_pages_evenly_and_leaves_no_small_tail() {
        // Never over 12 would make four decks: 11, 2, 11, 2.
        assert_eq!(breaks(&[11, 2, 11, 2], 12), [0..2, 2..4]);
        assert_eq!(breaks(&[5, 8, 5, 8], 12), [0..2, 2..4]);
        // 46 words over 4 pages at 30: 22 + 24, not 32 + 14.
        assert_eq!(breaks(&[10, 12, 10, 14], 30), [0..2, 2..4]);
        // 40 is under 1.5 × 30, so one deck.
        assert_eq!(breaks(&[10, 12, 10, 8], 30), [0..4]);
        // Twelve pages with eleven words are one batch.
        let mut sparse = vec![1; 11];
        sparse.extend([0, 12]);
        assert_eq!(breaks(&sparse, 12), [0..12, 12..13]);
    }

    #[test]
    fn a_crowded_page_stands_alone_and_shared_words_count_once() {
        assert_eq!(breaks(&[4, 40, 4], 12), [0..1, 1..2, 2..3]);
        let same = vec![vec![1, 2, 3, 4, 5, 6]; 4];
        // A word on four pages is one card.
        assert_eq!(even(&same, 6), [0..4]);
        assert_eq!(breaks(&[0; 45], 12), [0..20, 20..40, 40..45]);
        assert!(even(&[], 12).is_empty());
        assert_eq!(breaks(&[3], 0), [0..1]);
    }

    #[test]
    fn words_batches_start_at_the_page_and_count_their_words() {
        let words = pages_of(&[9, 6, 6, 12]);
        let b = plan_words(&words, &[100; 4], 12, 1);
        assert_eq!(spans(&b), [(1, 2), (3, 3)]);
        assert_eq!((b[0].index, b[0].words, b[0].chars), (0, 12, 200));
        assert_eq!((b[1].index, b[1].words), (1, 12));
    }

    #[test]
    fn light_pages_still_stop_at_ten_and_odd_sizes_hold() {
        assert_eq!(
            spans(&plan(&[10; 25], pages(3))),
            [(0, 9), (10, 19), (20, 24)]
        );
        assert!(plan(&[], pages(3)).is_empty());
        assert_eq!(spans(&plan(&[400; 3], pages(0))), [(0, 0), (1, 1), (2, 2)]);
        assert_eq!(spans(&plan(&[usize::MAX; 2], pages(5))), [(0, 1)]);
    }

    #[test]
    fn progress_rounds_up_to_its_batch_and_the_gate_follows() {
        let b = plan(&[400; 9], pages(3));
        assert_eq!(retypeset(4, &b, 9), 6);
        assert_eq!(retypeset(0, &b, 9), 0);
        assert_eq!(retypeset(9, &b, 9), 9);
        assert_eq!(retypeset(40, &b, 9), 9);
        assert_eq!(gate(&b, 3).map(|g| g.index), Some(1));
        assert_eq!(gate(&b, 9), None);
    }

    #[test]
    fn batches_start_at_the_page_the_mode_turns_on() {
        let b = plan_from(&[400; 10], pages(5), 2);
        assert_eq!(spans(&b), [(2, 6), (7, 9)]);
        assert_eq!((b[0].index, b[1].index), (0, 1));
        assert_eq!(b[0].label(), "pages 3–7");
        assert!(plan_from(&[400; 3], pages(5), 7).is_empty());
    }

    #[test]
    fn a_batch_names_its_pages_from_one() {
        let b = plan(&[400; 6], pages(3));
        assert_eq!(b[0].label(), "pages 1–3");
        assert_eq!(plan(&[400], pages(3))[0].label(), "page 1");
    }
}
