//! Local-first preview retypesetting and cooperative cancellation for [`Book`].

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use mareader_reflow::{Anchor, Block, Content, Flow, Fonts, Typography};

use super::{Book, SpineRange};
use crate::splice::{Inserts, Spliced, next_layout};

/// Books at or below both limits retypeset whole without a preview phase.
pub(crate) const SMALL_RETYPESET_BLOCKS: usize = 96;
const SMALL_RETYPESET_WEIGHT: usize = 48_000;
/// Target block and byte budget for an instant local-window preview.
const RETYPE_WINDOW_BLOCKS: usize = 48;
const RETYPE_WINDOW_WEIGHT: usize = 24_000;
/// Reach before and after the focus when slicing inside a large chapter.
const SLICE_BEFORE_WEIGHT: usize = 8_000;
const SLICE_AFTER_WEIGHT: usize = 12_000;
const SLICE_BEFORE_MIN_BLOCKS: usize = 2;
const SLICE_AFTER_MIN_BLOCKS: usize = 3;
const SLICE_BEFORE_MAX_BLOCKS: usize = 40;
const SLICE_AFTER_MAX_BLOCKS: usize = 56;
/// Max blocks scanned outward for a clean chapter-internal cut boundary.
const BOUNDARY_SCAN: usize = 24;

struct FlowState {
    flow: Arc<Flow>,
    layout: u64,
    inserts: Arc<Inserts>,
    spliced: Option<Arc<Spliced>>,
    full_spine: bool,
    windowed: bool,
}

impl Book {
    /// Whether every parsed block of the book is already in memory.
    pub fn has_all_blocks(&self) -> bool {
        self.full_spine
    }

    /// Whether this snapshot only laid out a local window of its blocks.
    pub fn is_windowed(&self) -> bool {
        self.windowed
    }

    /// Whether retypesetting to `typo` benefits from a local preview first.
    pub fn can_preview_reset(&self, typo: &Typography) -> bool {
        exceeds_small(&self.blocks) && (*typo != self.typo || self.windowed)
    }

    /// Full retypeset, stopping early if `cancel` is raised.
    pub fn reset_cancellable(
        &self,
        typo: &Typography,
        inserts: Option<&Arc<Inserts>>,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        self.reset_inner(typo, inserts, Some(cancel))
    }

    /// Local-window retypeset around `focus`; `None` if whole or cancelled.
    pub fn reset_preview(
        &self,
        typo: &Typography,
        inserts: Option<&Arc<Inserts>>,
        focus: Anchor,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let range = preview_range(&self.blocks, &self.spine, focus)?;
        let build = || {
            let language = self.language.as_deref();
            let fonts = self.flow.clone_fonts();
            let flow = set_window(&self.blocks, &range, typo, language, fonts, cancel)?;
            let flow = Arc::new(flow.keep_pictures(&self.flow));
            let inserts = Arc::clone(inserts.unwrap_or(&self.inserts));
            let local = filter_inserts(&inserts, &range);
            let spliced = (!local.list.is_empty())
                .then(|| Spliced::build(&flow, &self.blocks, &local, (None, &self.faces)))
                .flatten()
                .map(Arc::new);
            let state = FlowState {
                flow,
                layout: next_layout(),
                inserts,
                spliced,
                full_spine: self.full_spine,
                windowed: true,
            };
            Some(self.with_flow(typo, state))
        };
        catch_unwind(AssertUnwindSafe(build)).ok().flatten()
    }

    pub(super) fn reset_inner(
        &self,
        typo: &Typography,
        inserts: Option<&Arc<Inserts>>,
        cancel: Option<&AtomicBool>,
    ) -> Option<Self> {
        let build = || {
            let same = *typo == self.typo && !self.windowed;
            let (flow, layout) = match same {
                true => (Arc::clone(&self.flow), self.layout),
                false => {
                    let language = self.language.as_deref();
                    let fonts = self.flow.clone_fonts();
                    let laid = match cancel {
                        Some(flag) => set_cancellable(&self.blocks, typo, language, fonts, flag)?,
                        None => set_with_fonts(&self.blocks, typo, language, fonts),
                    };
                    (Arc::new(laid.keep_pictures(&self.flow)), next_layout())
                }
            };
            let inserts = Arc::clone(inserts.unwrap_or(&self.inserts));
            let old = self.spliced.as_deref().filter(|_| same);
            let spliced = (!inserts.list.is_empty())
                .then(|| Spliced::build(&flow, &self.blocks, &inserts, (old, &self.faces)))
                .flatten()
                .map(Arc::new);
            let state = FlowState {
                flow,
                layout,
                inserts,
                spliced,
                full_spine: self.full_spine,
                windowed: false,
            };
            Some(self.with_flow(typo, state))
        };
        catch_unwind(AssertUnwindSafe(build)).ok().flatten()
    }

    fn with_flow(&self, typo: &Typography, state: FlowState) -> Self {
        Self {
            identity: Arc::clone(&self.identity),
            blocks: Arc::clone(&self.blocks),
            flow: state.flow,
            typo: typo.clone(),
            layout: state.layout,
            inserts: state.inserts,
            spliced: state.spliced,
            refs: Arc::clone(&self.refs),
            spine: Arc::clone(&self.spine),
            faces: Arc::clone(&self.faces),
            notes: Arc::clone(&self.notes),
            toc: Arc::clone(&self.toc),
            title: self.title.clone(),
            author: self.author.clone(),
            language: self.language.clone(),
            rtl: self.rtl,
            lead: self.lead,
            archive: self.archive.clone(),
            narration: Arc::clone(&self.narration),
            read: self.read.clone(),
            full_spine: state.full_spine,
            windowed: state.windowed,
        }
    }
}

fn effective_typo(typo: &Typography, language: Option<&str>) -> Typography {
    match language {
        Some(tag) if *tag != typo.language => Typography {
            language: tag.to_owned(),
            ..typo.clone()
        },
        _ => typo.clone(),
    }
}

pub(super) fn set_with_fonts(
    blocks: &Arc<Vec<Block>>,
    typo: &Typography,
    language: Option<&str>,
    fonts: Fonts,
) -> Flow {
    let typo = effective_typo(typo, language);
    Flow::new(Arc::clone(blocks), &typo, fonts)
}

pub(super) fn set_cancellable(
    blocks: &Arc<Vec<Block>>,
    typo: &Typography,
    language: Option<&str>,
    fonts: Fonts,
    cancel: &AtomicBool,
) -> Option<Flow> {
    let typo = effective_typo(typo, language);
    Flow::new_cancellable(Arc::clone(blocks), &typo, fonts, cancel)
}

fn set_window(
    blocks: &Arc<Vec<Block>>,
    range: &Range<usize>,
    typo: &Typography,
    language: Option<&str>,
    fonts: Fonts,
    cancel: &AtomicBool,
) -> Option<Flow> {
    let typo = effective_typo(typo, language);
    Flow::window(
        Arc::clone(blocks),
        range.clone(),
        &typo,
        fonts,
        Some(cancel),
    )
}

fn filter_inserts(inserts: &Inserts, range: &Range<usize>) -> Inserts {
    if inserts.list.is_empty() {
        return Inserts::default();
    }
    let list = inserts
        .list
        .iter()
        .filter(|i| range.contains(&i.from.block) && range.contains(&i.to.block))
        .cloned()
        .collect();
    Inserts {
        key: inserts.key,
        list,
    }
}

fn block_weight(block: &Block) -> usize {
    match &block.content {
        Content::Text(t) => t.text.len().saturating_add(128),
        Content::Table(t) => t
            .rows
            .iter()
            .flat_map(|r| &r.cells)
            .map(|c| c.text.len().saturating_add(128))
            .sum(),
        Content::Image { .. } | Content::Rule | Content::Space(_) => 256,
    }
}

fn range_weight(blocks: &[Block], range: &Range<usize>, cap: usize) -> usize {
    let mut weight = 0usize;
    for b in blocks.get(range.clone()).unwrap_or_default() {
        weight = weight.saturating_add(block_weight(b));
        if weight > cap {
            break;
        }
    }
    weight
}

fn exceeds_small(blocks: &[Block]) -> bool {
    if blocks.len() > SMALL_RETYPESET_BLOCKS {
        return true;
    }
    let weight = range_weight(blocks, &(0..blocks.len()), SMALL_RETYPESET_WEIGHT);
    weight > SMALL_RETYPESET_WEIGHT
}

/// Picks a compact block slice around `focus`, preferring chapter bounds.
pub(crate) fn preview_range(
    blocks: &[Block],
    spine: &[SpineRange],
    focus: Anchor,
) -> Option<Range<usize>> {
    if !exceeds_small(blocks) {
        return None;
    }
    let idx = spine.iter().position(|s| s.blocks.contains(&focus.block));
    let range = match idx {
        Some(i) => {
            let chapter = spine.get(i).map(|s| s.blocks.clone())?;
            let weight = range_weight(blocks, &chapter, RETYPE_WINDOW_WEIGHT);
            if chapter.len() <= RETYPE_WINDOW_BLOCKS && weight <= RETYPE_WINDOW_WEIGHT {
                expand_chapters(blocks, spine, i, weight)
            } else {
                slice_giant(blocks, chapter, focus.block)
            }
        }
        None => {
            let clamped = focus.block.min(blocks.len().saturating_sub(1));
            slice_giant(blocks, 0..blocks.len(), clamped)
        }
    };
    if range.is_empty() || range.len() * 5 >= blocks.len() * 4 {
        return None;
    }
    Some(range)
}

fn expand_chapters(
    blocks: &[Block],
    spine: &[SpineRange],
    center: usize,
    initial_weight: usize,
) -> Range<usize> {
    let (mut first, mut last) = (center, center);
    let mut count = spine.get(center).map_or(0, |s| s.blocks.len());
    let mut weight = initial_weight;
    loop {
        let mut grew = false;
        if first > 0
            && let Some(prev) = spine.get(first - 1)
            && count + prev.blocks.len() <= RETYPE_WINDOW_BLOCKS
        {
            let room = RETYPE_WINDOW_WEIGHT.saturating_sub(weight);
            let add = range_weight(blocks, &prev.blocks, room);
            if add <= room {
                first -= 1;
                count += prev.blocks.len();
                weight += add;
                grew = true;
            }
        }
        if let Some(next) = spine.get(last + 1)
            && count + next.blocks.len() <= RETYPE_WINDOW_BLOCKS
        {
            let room = RETYPE_WINDOW_WEIGHT.saturating_sub(weight);
            let add = range_weight(blocks, &next.blocks, room);
            if add <= room {
                last += 1;
                count += next.blocks.len();
                weight += add;
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    let start = spine.get(first).map_or(0, |s| s.blocks.start);
    let end = spine.get(last).map_or(start, |s| s.blocks.end);
    start..end
}

fn is_clean_cut(block: &Block) -> bool {
    block.break_before
        || (block.frame.float.is_none()
            && block.frame.cell.is_none()
            && block.frame.boxes.is_empty())
}

fn slice_giant(blocks: &[Block], bounds: Range<usize>, focus: usize) -> Range<usize> {
    if bounds.is_empty() {
        return bounds;
    }
    let focus = focus.clamp(bounds.start, bounds.end.saturating_sub(1));
    let mut lo = focus;
    let (mut before_blocks, mut before_weight) = (0usize, 0usize);
    while lo > bounds.start && before_blocks < SLICE_BEFORE_MAX_BLOCKS {
        if before_blocks >= SLICE_BEFORE_MIN_BLOCKS && before_weight >= SLICE_BEFORE_WEIGHT {
            break;
        }
        lo -= 1;
        before_blocks += 1;
        before_weight = before_weight.saturating_add(blocks.get(lo).map_or(0, block_weight));
    }
    let mut hi = (focus + 1).min(bounds.end);
    let mut after_blocks = 0usize;
    let mut after_weight = blocks.get(focus).map_or(0, block_weight);
    while hi < bounds.end && after_blocks < SLICE_AFTER_MAX_BLOCKS {
        if after_blocks >= SLICE_AFTER_MIN_BLOCKS && after_weight >= SLICE_AFTER_WEIGHT {
            break;
        }
        after_weight = after_weight.saturating_add(blocks.get(hi).map_or(0, block_weight));
        hi += 1;
        after_blocks += 1;
    }
    let lo_floor = lo.saturating_sub(BOUNDARY_SCAN).max(bounds.start);
    while lo > lo_floor {
        if blocks.get(lo).is_some_and(is_clean_cut) {
            break;
        }
        lo -= 1;
    }
    let hi_ceil = hi.saturating_add(BOUNDARY_SCAN).min(bounds.end);
    while hi < hi_ceil {
        if blocks.get(hi).is_some_and(is_clean_cut) {
            break;
        }
        hi += 1;
    }
    lo..hi
}

#[cfg(test)]
mod tests {
    use mareader_reflow::Role;

    use super::*;
    use crate::sample;

    #[test]
    fn large_book_previews_locally_then_completes_with_matching_pages() {
        let bytes = Arc::new(sample::epub(80, "Large"));
        let base = Typography::default();
        let full = Book::open(bytes, &base).ok();
        assert!(full.is_some(), "sample opens");
        let Some(full) = full else {
            return;
        };
        assert!(full.blocks.len() > SMALL_RETYPESET_BLOCKS);
        let big = Typography {
            font_size: 24.0,
            ..base
        };
        assert!(full.can_preview_reset(&big));
        let focus = full.anchor(full.page_count() / 2).unwrap_or_default();
        let cancel = AtomicBool::new(false);
        let preview = full.reset_preview(&big, None, focus, &cancel);
        assert!(preview.is_some(), "large book builds local preview");
        let Some(preview) = preview else {
            return;
        };
        assert!(!preview.is_complete());
        assert!(preview.has_all_blocks());
        let local = preview.locate(focus);
        let complete = preview.reset_cancellable(&big, None, &cancel);
        assert!(complete.is_some(), "full layout finishes from preview");
        let Some(complete) = complete else {
            return;
        };
        assert!(complete.is_complete());
        assert!(preview.page_count() * 2 < complete.page_count());
        assert!(complete.page_count() > full.page_count());
        let expected_page = complete.locate(focus);
        assert_eq!(preview.same_page(local, &complete), Some(expected_page));
    }

    #[test]
    fn reset_cancellable_stops_when_cancelled() {
        let bytes = Arc::new(sample::epub(80, "Cancel"));
        let base = Typography::default();
        let Some(full) = Book::open(bytes, &base).ok() else {
            return;
        };
        let big = Typography {
            font_size: 24.0,
            ..base
        };
        let cancel = AtomicBool::new(true);
        assert!(full.reset_cancellable(&big, None, &cancel).is_none());
    }

    #[test]
    fn long_chapter_preview_stays_compact_by_text_weight() {
        let para = "Long literary paragraph ".repeat(80);
        let blocks: Vec<Block> = (0..60)
            .map(|_| Block::text(Role::Paragraph, &para))
            .collect();
        let spine = vec![SpineRange {
            item: 0,
            href: "ch1.xhtml".to_owned(),
            blocks: 0..60,
        }];
        let focus = Anchor {
            block: 30,
            offset: 0,
        };
        let range = preview_range(&blocks, &spine, focus);
        assert!(range.is_some(), "dense chapter slices locally");
        let Some(range) = range else {
            return;
        };
        assert!(range.contains(&30));
        assert!(range.len() < 20, "compact slice, got {}", range.len());
    }
}
