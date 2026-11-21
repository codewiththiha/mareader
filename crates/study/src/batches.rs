//! Study batches: whole pages, 1 to 10, planned from the pages' sizes.
//!
//! Pure, like the rest of `crates/study` — no UI, no AI, testable alone.
//!
//! Rules (see the plan, decision D3):
//!  1. A batch is whole pages, 1–10. Never a half page, never more than 10,
//!     whatever the character budget says.
//!  2. "Pages" mode counts pages that carry text, up to the quota.
//!  3. "Characters" mode is a budget converted back to whole pages by
//!     nearest-page rounding: a page joins when at least half of it fits.
//!  4. Light pages (a blank leaf, a lone heading, a few carried-over
//!     sentences) spend no quota; they ride along with a neighbour.
//!  5. A short tail merges into the batch before it when that stays ≤ 10 pages.

use serde::{Deserialize, Serialize};

/// A batch is never more than this many whole pages.
pub const MAX_PAGES: usize = 10;
/// Under this many characters a page is "light": it rides along instead of
/// spending a quota page.
pub const LIGHT_CHARS: usize = 140;
/// A tail under this share of the target merges into the batch before it.
const TAIL_SHARE: usize = 40; // per mille, 40 %

/// How a batch's size is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchUnit {
    #[default]
    Pages,
    Chars,
}

impl BatchUnit {
    pub const ALL: [BatchUnit; 2] = [BatchUnit::Pages, BatchUnit::Chars];

    pub fn label(self) -> &'static str {
        match self {
            Self::Pages => "Pages",
            Self::Chars => "Characters",
        }
    }
}

/// How big a batch may be.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchCfg {
    pub unit: BatchUnit,
    /// Pages mode: pages per batch, 1 to 10.
    pub pages: usize,
    /// Characters mode: characters per batch.
    pub chars: usize,
}

impl BatchCfg {
    pub const PAGES_MIN: usize = 1;
    pub const PAGES_MAX: usize = 10;
    pub const PAGES: usize = 3;

    pub const CHARS_MIN: usize = 500;
    pub const CHARS_MAX: usize = 8000;
    pub const CHARS: usize = 2400;

    pub fn sanitize(&mut self) {
        self.pages = self.pages.clamp(Self::PAGES_MIN, Self::PAGES_MAX);
        self.chars = self.chars.clamp(Self::CHARS_MIN, Self::CHARS_MAX);
    }
}

/// One planned batch: whole pages from `from` to `to`, inclusive.
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

impl Batch {
    /// `page 3` or `pages 3–7`, 1-based for the reader.
    pub fn label(&self) -> String {
        if self.from == self.to {
            format!("page {}", self.from + 1)
        } else {
            format!("pages {}–{}", self.from + 1, self.to + 1)
        }
    }
}

/// Plans whole-page batches from each page's character count.
///
/// Never splits a page: the lock always falls on a page boundary, so a font
/// size change re-plans batches from the text and never loses unlocked
/// progress — it rounds up to the end of its batch.
#[allow(clippy::unwrap_used)]
pub fn build_batches(sizes: &[usize], cfg: &BatchCfg) -> Vec<Batch> {
    let mut out: Vec<Batch> = Vec::new();
    let n = sizes.len();
    let quota = cfg
        .pages
        .clamp(BatchCfg::PAGES_MIN, BatchCfg::PAGES_MAX)
        .min(MAX_PAGES);
    let mut i = 0;
    while i < n {
        let mut j = i;
        let mut chars = 0usize;
        let mut heavy = 0usize;
        let mut light = 0usize;
        while j < n && j - i < MAX_PAGES {
            let s = sizes.get(j).copied().unwrap_or(0);
            let is_light = s < LIGHT_CHARS;
            if j > i && !is_light {
                match cfg.unit {
                    BatchUnit::Pages if heavy >= quota => break,
                    BatchUnit::Chars => {
                        let half = s / 2;
                        if chars.saturating_add(half) > cfg.chars {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            chars = chars.saturating_add(s);
            if is_light {
                light = light.saturating_add(1);
            } else {
                heavy = heavy.saturating_add(1);
            }
            j = j.saturating_add(1);
        }
        let mut notes = Vec::new();
        if light > 0 {
            notes.push(format!(
                "{} short page{} ride along",
                light,
                if light > 1 { "s" } else { "" }
            ));
        }
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
    // A short tail joins the batch before it, if there is room.
    if out.len() > 1 {
        let last_index = out.len() - 1;
        let last = out.get(last_index).unwrap().clone();
        let prev_index = last_index - 1;
        let prev = out.get(prev_index).unwrap().clone();
        let target = match cfg.unit {
            BatchUnit::Chars => cfg.chars,
            BatchUnit::Pages => prev.chars,
        };
        let short = last.chars.saturating_mul(100) < target.saturating_mul(TAIL_SHARE);
        let fits = prev.page_count.saturating_add(last.page_count) <= MAX_PAGES;
        if short && fits && prev_index > 0 {
            let merged = Batch {
                index: prev.index,
                from: prev.from,
                to: last.to,
                page_count: prev.page_count.saturating_add(last.page_count),
                chars: prev.chars.saturating_add(last.chars),
                notes: {
                    let mut n = prev.notes;
                    n.push("short tail merged".to_owned());
                    n
                },
            };
            out.truncate(prev_index);
            out.push(merged);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn first(batches: &[Batch]) -> Option<&Batch> {
        batches.first()
    }

    fn cfg(pages: usize, chars: usize) -> BatchCfg {
        BatchCfg {
            unit: BatchUnit::Pages,
            pages,
            chars,
        }
    }

    #[test]
    fn whole_pages_never_split_even_for_a_small_budget() {
        let sizes = vec![600, 600, 600];
        let batches = build_batches(&sizes, &cfg(1, 100));
        assert_eq!(batches.len(), 1);
        assert_eq!(first(&batches).map(|b| b.from), Some(0));
        assert_eq!(first(&batches).map(|b| b.to), Some(2));
    }

    #[test]
    fn pages_mode_respects_the_quota_and_the_ten_page_cap() {
        let sizes = vec![600; 12];
        let batches = build_batches(&sizes, &cfg(3, 10000));
        assert_eq!(batches.len(), 4);
        assert_eq!(batches.first().map(|b| b.to), Some(2));
        assert_eq!(batches.get(1).map(|b| b.from), Some(3));
        assert_eq!(batches.get(1).map(|b| b.to), Some(5));
        assert_eq!(batches.get(3).map(|b| b.from), Some(9));
        assert_eq!(batches.get(3).map(|b| b.to), Some(11));
    }

    #[test]
    fn characters_mode_rounds_to_the_nearest_whole_page() {
        // Three 600-char pages, a 2,200-char budget → 4 pages (nearest page).
        let sizes = vec![600, 600, 600, 600, 600];
        let batches = build_batches(
            &sizes,
            &BatchCfg {
                unit: BatchUnit::Chars,
                pages: 1,
                chars: 2200,
            },
        );
        assert_eq!(batches.len(), 1);
        assert_eq!(batches.first().map(|b| b.to), Some(3));
    }

    #[test]
    fn a_half_page_is_not_enough_to_join_in_characters_mode() {
        let sizes = vec![600, 600];
        let batches = build_batches(
            &sizes,
            &BatchCfg {
                unit: BatchUnit::Chars,
                pages: 1,
                chars: 700,
            },
        );
        // 600 alone fits; a second page needs 300 more but 600+300=900 > 700.
        assert_eq!(batches.len(), 1);
        assert_eq!(batches.first().map(|b| b.to), Some(0));
    }

    #[test]
    fn light_pages_ride_along_instead_of_spending_quota() {
        let sizes = vec![600, 80, 600, 600];
        let batches = build_batches(&sizes, &cfg(2, 10000));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches.first().map(|b| b.to), Some(3));
    }

    #[test]
    fn a_short_tail_merges_into_the_batch_before_it() {
        let sizes = vec![600, 600, 600, 60, 60];
        let batches = build_batches(&sizes, &cfg(3, 10000));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches.first().map(|b| b.to), Some(4));
        assert!(
            batches
                .first()
                .map(|b| b.notes.iter().any(|n| n == "short tail merged"))
                .unwrap_or(false)
        );
    }

    #[test]
    fn a_tail_too_big_to_merge_stays_its_own_batch() {
        let sizes = vec![600, 600, 600, 500];
        let batches = build_batches(&sizes, &cfg(3, 10000));
        assert_eq!(batches.len(), 2);
        assert_eq!(batches.first().map(|b| b.to), Some(2));
        assert_eq!(batches.get(1).map(|b| b.from), Some(3));
    }

    #[test]
    fn the_cap_is_never_exceeded_when_merging_a_tail() {
        let sizes = vec![600; 10];
        let batches = build_batches(&sizes, &cfg(10, 10000));
        assert_eq!(batches.len(), 1);
        assert_eq!(batches.first().map(|b| b.page_count), Some(10));
    }
}
