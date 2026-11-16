//! Local-first preview retypesetting and cooperative cancellation for [`Book`].

use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use mareader_reflow::{Anchor, Block, BookFace, Flow, Fonts, Typography};

use super::{Book, SpineRange, set};
use crate::splice::{Inserts, Spliced, next_layout};

/// Books at or below this block count retypeset whole without a preview phase.
pub(crate) const SMALL_RETYPESET_BLOCKS: usize = 450;
/// Target block budget for an instant local-window preview retypeset.
const RETYPE_WINDOW_BLOCKS: usize = 350;
/// Block reach before and after the focus when slicing inside a giant chapter.
const GIANT_BEFORE: usize = 100;
const GIANT_AFTER: usize = 220;
/// Max blocks scanned outward for a clean giant-chapter cut boundary.
const BOUNDARY_SCAN: usize = 64;

struct FlowState {
    flow: Arc<Flow>,
    layout: u64,
    inserts: Arc<Inserts>,
    spliced: Option<Arc<Spliced>>,
    complete: bool,
    windowed: bool,
}

impl Book {
    /// Whether every parsed block of the book is already in memory.
    pub fn has_all_blocks(&self) -> bool {
        self.complete || self.windowed
    }

    /// Whether retypesetting to `typo` benefits from a local preview first.
    pub fn can_preview_reset(&self, typo: &Typography) -> bool {
        self.has_all_blocks()
            && self.blocks.len() > SMALL_RETYPESET_BLOCKS
            && (*typo != self.typo || self.windowed)
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
            let flow = set_window(
                &self.blocks,
                range.clone(),
                typo,
                language,
                &self.faces,
                cancel,
            )?;
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
                complete: false,
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
                    let laid = match cancel {
                        Some(flag) => {
                            set_cancellable(&self.blocks, typo, language, &self.faces, flag)?
                        }
                        None => set(&self.blocks, typo, language, &self.faces),
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
                complete: self.has_all_blocks(),
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
            complete: state.complete,
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

fn set_cancellable(
    blocks: &Arc<Vec<Block>>,
    typo: &Typography,
    language: Option<&str>,
    faces: &[BookFace],
    cancel: &AtomicBool,
) -> Option<Flow> {
    let fonts = Fonts::system().with_faces(faces);
    let typo = effective_typo(typo, language);
    Flow::new_cancellable(Arc::clone(blocks), &typo, fonts, cancel)
}

fn set_window(
    blocks: &Arc<Vec<Block>>,
    range: Range<usize>,
    typo: &Typography,
    language: Option<&str>,
    faces: &[BookFace],
    cancel: &AtomicBool,
) -> Option<Flow> {
    let fonts = Fonts::system().with_faces(faces);
    let typo = effective_typo(typo, language);
    Flow::window(Arc::clone(blocks), range, &typo, fonts, Some(cancel))
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

/// Picks a compact block slice around `focus`, preferring chapter bounds.
pub(crate) fn preview_range(
    blocks: &[Block],
    spine: &[SpineRange],
    focus: Anchor,
) -> Option<Range<usize>> {
    if blocks.len() <= SMALL_RETYPESET_BLOCKS {
        return None;
    }
    let idx = spine.iter().position(|s| s.blocks.contains(&focus.block));
    let range = match idx {
        Some(i) => {
            let chapter = spine.get(i).map(|s| s.blocks.clone())?;
            if chapter.len() <= RETYPE_WINDOW_BLOCKS {
                expand_chapters(spine, i)
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

fn expand_chapters(spine: &[SpineRange], center: usize) -> Range<usize> {
    let (mut first, mut last) = (center, center);
    let mut count = spine.get(center).map_or(0, |s| s.blocks.len());
    loop {
        let mut grew = false;
        if first > 0
            && let Some(prev) = spine.get(first - 1)
            && count + prev.blocks.len() <= RETYPE_WINDOW_BLOCKS
        {
            first -= 1;
            count += prev.blocks.len();
            grew = true;
        }
        if let Some(next) = spine.get(last + 1)
            && count + next.blocks.len() <= RETYPE_WINDOW_BLOCKS
        {
            last += 1;
            count += next.blocks.len();
            grew = true;
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
    let mut lo = focus.saturating_sub(GIANT_BEFORE).max(bounds.start);
    let mut hi = focus.saturating_add(GIANT_AFTER).min(bounds.end);
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
}
