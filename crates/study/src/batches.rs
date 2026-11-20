//! Turning a book's page sizes into whole-page study batches.

use serde::{Deserialize, Serialize};

use crate::mode::BatchUnit;

/// A batch never holds more pages than this, whatever the budget says.
pub const MAX_PAGES: usize = 10;
/// A page shorter than this rides with its neighbour instead of using quota.
pub const LIGHT_CHARS: usize = 140;

/// The planner's knobs: unit, page quota or character budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchCfg {
    pub unit: BatchUnit,
    pub pages: usize,
    pub chars: usize,
}

/// One batch: a half-open page range with the text it covers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Batch {
    pub index: usize,
    /// First page, 0-based.
    pub from: usize,
    /// Last page, inclusive.
    pub to: usize,
    pub page_count: usize,
    pub chars: usize,
    pub notes: Vec<String>,
}

/// Splits `sizes` (characters per page) into batches under `cfg`.
///
/// Whole pages only, 1–10. Light pages ride along; a short tail merges into
/// the batch before it when that stays within the cap.
pub fn build_batches(sizes: &[usize], cfg: &BatchCfg) -> Vec<Batch> {
    let mut out = Vec::new();
    let quota = MAX_PAGES.min(cfg.pages.max(1));
    let n = sizes.len();
    let mut i = 0;
    while i < n {
        let (j, chars, light) = fill(sizes, i, quota, cfg);
        let notes = ride_along(light);
        out.push(Batch {
            index: out.len(),
            from: i,
            to: j.saturating_sub(1),
            page_count: j - i,
            chars,
            notes,
        });
        i = j;
    }
    merge_short_tail(&mut out, cfg);
    out
}

/// How far this batch reaches from `start`, and how light its pages were.
fn fill(sizes: &[usize], start: usize, quota: usize, cfg: &BatchCfg) -> (usize, usize, usize) {
    let mut j = start;
    let mut chars = 0;
    let mut heavy = 0;
    let mut light = 0;
    while j < sizes.len() && j - start < MAX_PAGES {
        let s = sizes.get(j).copied().unwrap_or(0);
        let is_light = s < LIGHT_CHARS;
        if j > start && !is_light && over_quota(cfg, heavy, chars, s, quota) {
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
    (j.max(start + 1), chars, light)
}

/// Whether adding a heavy page would pass the unit's limit.
fn over_quota(cfg: &BatchCfg, heavy: usize, chars: usize, next: usize, quota: usize) -> bool {
    match cfg.unit {
        BatchUnit::Pages => heavy >= quota,
        BatchUnit::Chars => {
            // Nearest-page rounding: a page joins when half of it fits.
            chars.saturating_add(next / 2) > cfg.chars
        }
    }
}

fn ride_along(light: usize) -> Vec<String> {
    if light == 0 {
        return Vec::new();
    }
    let word = if light == 1 { "page" } else { "pages" };
    vec![format!("{light} short {word} ride along")]
}

/// Folds a last batch under 40% of target into the one before it.
fn merge_short_tail(out: &mut Vec<Batch>, cfg: &BatchCfg) {
    if out.len() < 2 {
        return;
    }
    let last = match out.last() {
        Some(b) => b.clone(),
        None => return,
    };
    let prev = match out.get(out.len().wrapping_sub(2)) {
        Some(b) => b.clone(),
        None => return,
    };
    let target = match cfg.unit {
        BatchUnit::Chars => cfg.chars,
        BatchUnit::Pages => prev.chars,
    };
    let under = last.chars * 5 < target * 2;
    let fits = prev.page_count + last.page_count <= MAX_PAGES;
    if under && fits {
        let merged = Batch {
            index: prev.index,
            from: prev.from,
            to: last.to,
            page_count: prev.page_count + last.page_count,
            chars: prev.chars.saturating_add(last.chars),
            notes: {
                let mut notes = prev.notes;
                notes.push("short tail merged".to_owned());
                notes
            },
        };
        let len = out.len();
        out.truncate(len - 1);
        out.pop();
        out.push(merged);
    }
}

/// "page 3" or "pages 3–5", 1-based as the reader counts.
pub fn range_label(batch: &Batch) -> String {
    if batch.from == batch.to {
        format!("page {}", batch.from + 1)
    } else {
        format!("pages {}–{}", batch.from + 1, batch.to + 1)
    }
}

/// The batch holding `page`, if any.
pub fn batch_of(batches: &[Batch], page: usize) -> Option<&Batch> {
    batches.iter().find(|b| b.from <= page && page <= b.to)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn cfg(unit: BatchUnit, pages: usize, chars: usize) -> BatchCfg {
        BatchCfg { unit, pages, chars }
    }

    fn sizes_of(count: usize, each: usize) -> Vec<usize> {
        vec![each; count]
    }

    #[test]
    fn pages_mode_counts_heavy_pages_to_the_quota() {
        let batches = build_batches(&sizes_of(7, 400), &cfg(BatchUnit::Pages, 3, 0));
        // 3 + 3 + a one-page tail that merges back into the second.
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].page_count, 3);
        assert_eq!(batches[1].page_count, 4);
        assert!(batches[1].notes.iter().any(|n| n.contains("merged")));
    }

    #[test]
    fn light_pages_ride_and_do_not_use_quota() {
        let mut sizes = sizes_of(3, 400);
        sizes.push(LIGHT_CHARS - 1);
        sizes.extend(sizes_of(2, 400));
        let batches = build_batches(&sizes, &cfg(BatchUnit::Pages, 3, 0));
        assert_eq!(batches[0].page_count, 4);
        assert!(batches[0].notes[0].contains("ride along"));
    }

    #[test]
    fn chars_mode_rounds_to_whole_pages() {
        // 2200 budget over 600-char pages: half of page 4 fits → 4 pages.
        let batches = build_batches(&sizes_of(8, 600), &cfg(BatchUnit::Chars, 0, 2_200));
        assert_eq!(batches[0].page_count, 4);
    }

    #[test]
    fn a_batch_is_never_over_ten_pages() {
        let batches = build_batches(&sizes_of(25, 50), &cfg(BatchUnit::Chars, 0, 8_000));
        assert!(batches.iter().all(|b| b.page_count <= MAX_PAGES));
    }

    #[test]
    fn labels_are_one_based() {
        let batches = build_batches(&sizes_of(4, 400), &cfg(BatchUnit::Pages, 2, 0));
        assert_eq!(range_label(&batches[0]), "pages 1–2");
        let one = Batch {
            index: 0,
            from: 2,
            to: 2,
            page_count: 1,
            chars: 10,
            notes: vec![],
        };
        assert_eq!(range_label(&one), "page 3");
    }

    #[test]
    fn batch_of_finds_the_page() {
        let batches = build_batches(&sizes_of(6, 400), &cfg(BatchUnit::Pages, 3, 0));
        assert_eq!(batch_of(&batches, 0).map(|b| b.index), Some(0));
        assert_eq!(batch_of(&batches, 4).map(|b| b.index), Some(1));
        assert!(batch_of(&batches, 99).is_none());
    }
}
