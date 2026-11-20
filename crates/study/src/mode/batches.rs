//! The batch planner: a book's pages as study batches (decision D3).
//!
//! A batch is whole pages, 1 to [`MAX_PAGES`]. Pages mode counts pages that
//! carry text; characters mode spends a budget, and a page joins when at least
//! half of it fits. Light pages (a blank leaf, a lone heading) use no quota and
//! ride with a neighbour. A short tail joins the batch before it while that
//! stays within [`MAX_PAGES`].

use serde::{Deserialize, Serialize};

pub const MAX_PAGES: usize = 10;
/// A page with fewer characters is light.
pub const LIGHT_CHARS: usize = 140;

/// What a batch's size is counted in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Unit {
    #[default]
    Pages,
    Chars,
}

/// How big a batch should be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub unit: Unit,
    pub pages: usize,
    pub chars: usize,
}

/// Pages `from` to `to`, both included, counted from zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Batch {
    pub index: usize,
    pub from: usize,
    pub to: usize,
    pub chars: usize,
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
}

/// The batches of pages with `sizes` characters each, in order.
pub fn plan(sizes: &[usize], size: Size) -> Vec<Batch> {
    let quota = size.pages.clamp(1, MAX_PAGES);
    let mut out: Vec<Batch> = Vec::new();
    let mut i = 0;
    while i < sizes.len() {
        let (mut j, mut chars, mut heavy, mut light) = (i, 0usize, 0usize, 0usize);
        while j < sizes.len() && j - i < MAX_PAGES {
            let s = sizes.get(j).copied().unwrap_or(0);
            let is_light = s < LIGHT_CHARS;
            if j > i && !is_light {
                let full = match size.unit {
                    Unit::Pages => heavy >= quota,
                    // Joins while at least half of it fits.
                    Unit::Chars => {
                        chars.saturating_mul(2).saturating_add(s) > size.chars.saturating_mul(2)
                    }
                };
                if full {
                    break;
                }
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
            light,
            merged: false,
        });
        i = j.max(i + 1);
    }
    merge_tail(&mut out, size);
    out
}

/// A tail under 40% of the target joins the batch before it.
fn merge_tail(out: &mut Vec<Batch>, size: Size) {
    let n = out.len();
    let (Some(last), Some(prev)) = (
        n.checked_sub(1).and_then(|i| out.get(i)).cloned(),
        n.checked_sub(2).and_then(|i| out.get(i)),
    ) else {
        return;
    };
    let target = match size.unit {
        Unit::Chars => size.chars,
        Unit::Pages => prev.chars,
    };
    let short = last.chars.saturating_mul(5) < target.saturating_mul(2);
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

/// Progress when the mode turns on at `current`: pages behind are free.
pub fn entering(current: usize, unlocked: usize, batches: &[Batch]) -> usize {
    match batches.iter().find(|b| b.holds(current)) {
        Some(b) if current >= unlocked && current == b.from => b.from,
        Some(b) if current >= unlocked => b.to + 1,
        _ => unlocked,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn pages(n: usize) -> Size {
        Size {
            unit: Unit::Pages,
            pages: n,
            chars: 0,
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

    #[test]
    fn chars_mode_takes_a_page_when_half_of_it_fits() {
        let size = Size {
            unit: Unit::Chars,
            pages: 3,
            chars: 1000,
        };
        assert_eq!(spans(&plan(&[400; 6], size)), [(0, 2), (3, 5)]);
        let size = Size {
            unit: Unit::Chars,
            pages: 3,
            chars: 900,
        };
        assert_eq!(spans(&plan(&[400; 6], size)), [(0, 1), (2, 3), (4, 5)]);
    }

    #[test]
    fn light_pages_still_stop_at_ten_and_odd_sizes_hold() {
        assert_eq!(spans(&plan(&[10; 25], pages(3))), [(0, 9), (10, 19), (20, 24)]);
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
    fn turning_on_mid_batch_frees_the_rest_of_it() {
        let b = plan(&[400; 9], pages(3));
        assert_eq!(entering(4, 0, &b), 6);
        assert_eq!(entering(3, 0, &b), 3);
        assert_eq!(entering(1, 6, &b), 6);
        assert_eq!(entering(99, 2, &b), 2);
    }
}
