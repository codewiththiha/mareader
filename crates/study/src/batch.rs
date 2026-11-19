//! Study batches: whole pages chosen by pages or by a character budget.

use serde::{Deserialize, Serialize};

/// Most pages one batch may hold, whatever the budget says.
pub const MAX_PAGES: usize = 10;
/// Pages shorter than this ride along instead of using a quota place.
pub const LIGHT_CHARS: usize = 140;

/// How a batch size is chosen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchUnit {
    #[default]
    Pages,
    Chars,
}

/// A batch size as the reader chose it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct BatchCfg {
    pub unit: BatchUnit,
    /// Pages per batch, 1 to [`MAX_PAGES`].
    pub pages: usize,
    /// Character budget, converted back to whole pages.
    pub chars: usize,
}

impl Default for BatchCfg {
    fn default() -> Self {
        Self {
            unit: BatchUnit::Pages,
            pages: 5,
            chars: 2400,
        }
    }
}

impl BatchCfg {
    /// Pulls every value into its range.
    pub fn sanitized(mut self) -> Self {
        self.pages = self.pages.clamp(1, MAX_PAGES);
        self.chars = self.chars.clamp(100, 80_000);
        self
    }
}

/// One batch: a run of whole pages.
#[derive(Clone, Debug, PartialEq, Eq)]
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

/// Splits page character `sizes` into batches by `cfg`.
pub fn build_batches(sizes: &[usize], cfg: BatchCfg) -> Vec<Batch> {
    let cfg = cfg.sanitized();
    let mut out: Vec<Batch> = Vec::new();
    let mut i = 0usize;
    while i < sizes.len() {
        let mut j = i;
        let mut chars = 0usize;
        let mut heavy = 0usize;
        let mut light = 0usize;
        while j < sizes.len() && j.saturating_sub(i) < MAX_PAGES {
            let s = sizes.get(j).copied().unwrap_or(0);
            let is_light = s < LIGHT_CHARS;
            if j > i && !is_light {
                if cfg.unit == BatchUnit::Pages && heavy >= cfg.pages {
                    break;
                }
                if cfg.unit == BatchUnit::Chars && chars.saturating_add(s / 2) > cfg.chars {
                    break;
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
        if j == i {
            j = i.saturating_add(1);
        }
        let mut notes = Vec::new();
        if light > 0 {
            notes.push(format!(
                "{light} short page{} ride along",
                if light > 1 { "s" } else { "" }
            ));
        }
        out.push(Batch {
            index: out.len(),
            from: i,
            to: j.saturating_sub(1),
            page_count: j.saturating_sub(i),
            chars,
            notes,
        });
        i = j;
    }
    merge_short_tail(&mut out, cfg);
    out
}

fn merge_short_tail(out: &mut Vec<Batch>, cfg: BatchCfg) {
    if out.len() < 2 {
        return;
    }
    let Some(last) = out.last().cloned() else {
        return;
    };
    let Some(prev) = out.get(out.len().saturating_sub(2)).cloned() else {
        return;
    };
    let short = match cfg.unit {
        BatchUnit::Chars => {
            (last.chars as f64) < 0.4 * (cfg.chars as f64)
                && (prev.chars.saturating_add(last.chars) as f64) <= 1.25 * (cfg.chars as f64)
        }
        BatchUnit::Pages => (last.page_count as f64) < 0.4 * (cfg.pages as f64),
    };
    if short && prev.page_count.saturating_add(last.page_count) <= MAX_PAGES {
        let mut notes = prev.notes.clone();
        notes.push("short tail merged".to_owned());
        let merged = Batch {
            index: prev.index,
            from: prev.from,
            to: last.to,
            page_count: prev.page_count.saturating_add(last.page_count),
            chars: prev.chars.saturating_add(last.chars),
            notes,
        };
        out.truncate(out.len().saturating_sub(2));
        out.push(merged);
        for (n, b) in out.iter_mut().enumerate() {
            b.index = n;
        }
    }
}

/// "page 4" or "pages 4–6" for a batch.
pub fn range_label(batch: &Batch) -> String {
    if batch.from == batch.to {
        format!("page {}", batch.from.saturating_add(1))
    } else {
        format!(
            "pages {}–{}",
            batch.from.saturating_add(1),
            batch.to.saturating_add(1)
        )
    }
}

/// Seconds a deck asks for: cards times seconds per card.
pub fn deck_seconds(cards: usize, sec_per_card: u32) -> u32 {
    (cards as u32).saturating_mul(sec_per_card.clamp(2, 20))
}

/// Seconds left after `known` cards were marked known.
pub fn seconds_after_known(total_secs: u32, known: usize, sec_per_card: u32) -> u32 {
    total_secs.saturating_sub((known as u32).saturating_mul(sec_per_card.clamp(2, 20)))
}

/// A duration as `m:ss`.
pub fn mmss(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_pages(pages: usize) -> BatchCfg {
        BatchCfg {
            unit: BatchUnit::Pages,
            pages,
            chars: 2400,
        }
    }

    #[test]
    fn pages_mode_counts_whole_pages() {
        let b = build_batches(&[500, 500, 500, 500, 500, 500, 500], cfg_pages(3));
        assert_eq!(b.len(), 2, "a one-page tail under 40% merges");
        assert_eq!(
            (b.first().map(|x| x.from), b.first().map(|x| x.to)),
            (Some(0), Some(2))
        );
    }

    #[test]
    fn light_pages_ride_along() {
        let b = build_batches(&[500, 20, 500, 500], cfg_pages(2));
        assert_eq!(b.len(), 2);
        assert_eq!(b.first().map(|x| x.page_count), Some(3));
    }

    #[test]
    fn chars_mode_rounds_to_nearest_page() {
        let cfg = BatchCfg {
            unit: BatchUnit::Chars,
            pages: 3,
            chars: 2200,
        };
        let b = build_batches(&[600, 600, 600, 600, 600], cfg);
        assert_eq!(b.first().map(|x| x.page_count), Some(4));
    }

    #[test]
    fn never_more_than_ten_pages() {
        let b = build_batches(&[600; 25], cfg_pages(10));
        assert!(b.iter().all(|x| x.page_count <= MAX_PAGES));
        assert_eq!(b.first().map(|x| x.page_count), Some(10));
    }

    #[test]
    fn a_short_tail_merges() {
        let b = build_batches(&[600, 600, 600, 100, 90], cfg_pages(3));
        assert_eq!(b.len(), 1);
    }

    #[test]
    fn timer_scales_and_known_cards_take_time_off() {
        assert_eq!(deck_seconds(12, 6), 72);
        assert_eq!(seconds_after_known(72, 2, 6), 60);
        assert_eq!(mmss(72), "1:12");
        let batch = Batch {
            index: 0,
            from: 3,
            to: 3,
            page_count: 1,
            chars: 9,
            notes: vec![],
        };
        assert_eq!(range_label(&batch), "page 4");
    }
}
