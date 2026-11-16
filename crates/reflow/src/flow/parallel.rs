//! Safe parallel block shaping, sub-range layout, and cooperative cancellation.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;

#[cfg(not(target_arch = "wasm32"))]
const PARALLEL_MIN_BLOCKS: usize = 600;
#[cfg(not(target_arch = "wasm32"))]
const MAX_PARALLEL_THREADS: usize = 4;

impl Flow {
    /// Reuses this flow's loaded system and book faces without re-parsing.
    pub fn clone_fonts(&self) -> Fonts {
        self.relay
            .fonts
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .fork()
    }

    pub(super) fn set_cut(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        mut fonts: Fonts,
        (width, height): (f32, f32),
        cut: f32,
    ) -> Self {
        let stacks = Stacks::new(&typo.faces);
        // Laid to measure, then let go; endless streams never turn.
        let turnable = cut.is_finite();
        let items: Vec<(usize, &Block)> = blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.is_blank())
            .collect();
        let set = measure_items(
            &items,
            typo,
            &stacks,
            &mut fonts,
            (width, height),
            turnable,
            None,
        )
        .unwrap_or_default();
        Self::finish(blocks, set, typo, (stacks, fonts), (width, height), cut)
    }

    /// Sets `blocks` on its page, stopping early when `cancel` is raised.
    pub fn new_cancellable(
        blocks: Arc<Vec<Block>>,
        typo: &Typography,
        fonts: Fonts,
        cancel: &AtomicBool,
    ) -> Option<Self> {
        let room = Card::of(typo, column_width(typo)).room();
        Self::set_range(blocks, None, typo, fonts, room, Some(cancel))
    }

    /// Sets only `blocks[range]`, keeping global indices against `blocks`.
    pub fn window(
        blocks: Arc<Vec<Block>>,
        range: Range<usize>,
        typo: &Typography,
        fonts: Fonts,
        cancel: Option<&AtomicBool>,
    ) -> Option<Self> {
        let room = Card::of(typo, column_width(typo)).room();
        Self::set_range(blocks, Some(range), typo, fonts, room, cancel)
    }

    fn set_range(
        blocks: Arc<Vec<Block>>,
        range: Option<Range<usize>>,
        typo: &Typography,
        mut fonts: Fonts,
        (width, height): (f32, f32),
        cancel: Option<&AtomicBool>,
    ) -> Option<Self> {
        if cancelled(cancel) {
            return None;
        }
        let stacks = Stacks::new(&typo.faces);
        let span = match range {
            Some(r) => r.start.min(blocks.len())..r.end.min(blocks.len()),
            None => 0..blocks.len(),
        };
        let items: Vec<(usize, &Block)> = blocks
            .get(span.clone())
            .unwrap_or_default()
            .iter()
            .enumerate()
            .map(|(off, b)| (span.start + off, b))
            .filter(|(_, b)| !b.is_blank())
            .collect();
        let set = measure_items(
            &items,
            typo,
            &stacks,
            &mut fonts,
            (width, height),
            true,
            cancel,
        )?;
        if cancelled(cancel) {
            return None;
        }
        let flow = Self::finish(blocks, set, typo, (stacks, fonts), (width, height), height);
        Some(flow)
    }
}

pub(super) fn cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel.is_some_and(|flag| flag.load(Ordering::Relaxed))
}

#[derive(Clone, Copy)]
struct GroupRoom {
    turned: bool,
    fixed: bool,
    column: f32,
    height: f32,
}

/// Measures every non-blank block, splitting large runs across cores when safe.
pub(super) fn measure_items(
    items: &[(usize, &Block)],
    typo: &Typography,
    stacks: &Stacks,
    fonts: &mut Fonts,
    (width, height): (f32, f32),
    turnable: bool,
    cancel: Option<&AtomicBool>,
) -> Option<Vec<Set>> {
    let mut out = Vec::with_capacity(items.len());
    // Vertical lines run down; fixed pages keep their viewport.
    let part = |b: &Block| (b.frame.turned, sheet_key(&b.frame));
    for group in items.chunk_by(|a, b| part(a.1) == part(b.1)) {
        if cancelled(cancel) {
            return None;
        }
        let first = group.first().map(|(_, b)| &b.frame);
        let sheet = first.and_then(|f| f.sheet.as_deref());
        let turned = turnable && sheet.is_none() && first.is_some_and(|f| f.turned);
        let (column, group_h) = match sheet {
            Some(s) => (s.width, s.height),
            None if turned => (height, width),
            None => (width, height),
        };
        let room = GroupRoom {
            turned,
            fixed: sheet.is_some(),
            column,
            height: group_h,
        };
        let sets = measure_group(group, typo, stacks, fonts, room, cancel)?;
        out.extend(sets);
    }
    Some(out)
}

fn measure_group(
    group: &[(usize, &Block)],
    typo: &Typography,
    stacks: &Stacks,
    fonts: &mut Fonts,
    room: GroupRoom,
    cancel: Option<&AtomicBool>,
) -> Option<Vec<Set>> {
    #[cfg(not(target_arch = "wasm32"))]
    if !room.fixed && group.len() >= PARALLEL_MIN_BLOCKS {
        let workers = std::thread::available_parallelism().map_or(1, |n| {
            n.get().saturating_sub(2).clamp(1, MAX_PARALLEL_THREADS)
        });
        let slices = split_safe(group, workers);
        if slices.len() > 1 {
            return measure_parallel(&slices, typo, stacks, fonts, room, cancel);
        }
    }
    measure_slice(group, typo, stacks, fonts, room, cancel)
}

fn measure_slice(
    slice: &[(usize, &Block)],
    typo: &Typography,
    stacks: &Stacks,
    fonts: &mut Fonts,
    room: GroupRoom,
    cancel: Option<&AtomicBool>,
) -> Option<Vec<Set>> {
    let mut setter = Setter::new(typo, stacks, fonts);
    (setter.turned, setter.fixed) = (room.turned, room.fixed);
    (setter.column, setter.height) = (room.column, room.height);
    setter.set_all_cancellable(slice, cancel)
}

#[cfg(not(target_arch = "wasm32"))]
fn measure_parallel(
    slices: &[&[(usize, &Block)]],
    typo: &Typography,
    stacks: &Stacks,
    fonts: &Fonts,
    room: GroupRoom,
    cancel: Option<&AtomicBool>,
) -> Option<Vec<Set>> {
    let results: Vec<Option<Vec<Set>>> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(slices.len());
        for &slice in slices {
            let mut local = fonts.fork();
            handles.push(
                scope.spawn(move || measure_slice(slice, typo, stacks, &mut local, room, cancel)),
            );
        }
        handles
            .into_iter()
            .map(|handle| handle.join().ok().flatten())
            .collect()
    });
    let total: usize = results
        .iter()
        .filter_map(|r| r.as_ref().map(Vec::len))
        .sum();
    let mut out = Vec::with_capacity(total);
    for chunk in results {
        out.extend(chunk?);
    }
    Some(out)
}

/// Splits `group` into up to `parts` slices at float- and grid-free boundaries.
#[cfg(not(target_arch = "wasm32"))]
fn split_safe<'a, 'b>(
    group: &'a [(usize, &'b Block)],
    parts: usize,
) -> Vec<&'a [(usize, &'b Block)]> {
    if parts <= 1 || group.len() < 2 {
        return vec![group];
    }
    let mut safe = Vec::new();
    let mut open_float: Option<Holders> = None;
    for (k, &(_, block)) in group.iter().enumerate() {
        if let Some(f) = block.frame.float.as_ref() {
            open_float = Some(Holders::of(&block.frame, usize::from(f.depth)));
            continue;
        }
        if let Some(holders) = &open_float
            && (!holders.hold(&block.frame) || !matches!(block.content, Content::Text(_)))
        {
            open_float = None;
        }
        if k > 0 && open_float.is_none() && block.frame.cell.is_none() {
            safe.push(k);
        }
    }
    if safe.is_empty() {
        return vec![group];
    }
    let mut cuts = Vec::with_capacity(parts);
    let mut prev = 0usize;
    for i in 1..parts {
        let target = group.len() * i / parts;
        let at = safe.partition_point(|&k| k < target);
        let picked = match (at.checked_sub(1).and_then(|j| safe.get(j)), safe.get(at)) {
            (Some(&a), Some(&b)) => {
                if target.saturating_sub(a) <= b.saturating_sub(target) {
                    Some(a)
                } else {
                    Some(b)
                }
            }
            (Some(&a), None) => Some(a),
            (None, Some(&b)) => Some(b),
            (None, None) => None,
        };
        if let Some(cut) = picked.filter(|&c| c > prev && c < group.len()) {
            cuts.push(cut);
            prev = cut;
        }
    }
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut start = 0;
    for cut in cuts {
        if let Some(slice) = group.get(start..cut) {
            out.push(slice);
        }
        start = cut;
    }
    if let Some(tail) = group.get(start..)
        && !tail.is_empty()
    {
        out.push(tail);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn many_blocks(count: usize) -> Arc<Vec<Block>> {
        let mut blocks = Vec::with_capacity(count);
        for i in 0..count {
            let mut b = if i % 40 == 0 {
                let title = format!("Chapter {}", i / 40 + 1);
                let mut h = Block::text(Role::Heading(2), &title);
                h.break_before = i > 0;
                h
            } else {
                let body = format!("Paragraph {i} wraps cleanly across a couple of lines.");
                Block::text(Role::Paragraph, &body)
            };
            if i == 0 {
                b.break_before = false;
            }
            blocks.push(b);
        }
        Arc::new(blocks)
    }

    #[test]
    fn parallel_layout_matches_sequential_exactly() -> Result<(), &'static str> {
        let blocks = many_blocks(900);
        let typo = Typography::default();
        let room = Card::of(&typo, column_width(&typo)).room();
        let stacks = Stacks::new(&typo.faces);
        let items: Vec<(usize, &Block)> = blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.is_blank())
            .collect();
        let group_room = GroupRoom {
            turned: false,
            fixed: false,
            column: room.0,
            height: room.1,
        };
        let mut seq_fonts = Fonts::bundled();
        let seq_sets = measure_slice(&items, &typo, &stacks, &mut seq_fonts, group_room, None)
            .ok_or("sequential layout failed")?;
        let par = Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
        assert_eq!(seq_sets.len(), par.blocks.len());
        for (a, b) in seq_sets.iter().zip(par.blocks.iter()) {
            assert_eq!(
                (a.source, &a.units, &a.starts),
                (b.source, &b.units, &b.starts)
            );
        }
        Ok(())
    }

    #[test]
    fn window_layout_preserves_global_anchors_and_renders() -> Result<(), &'static str> {
        let blocks = many_blocks(800);
        let typo = Typography::default();
        let full = Flow::new(Arc::clone(&blocks), &typo, Fonts::bundled());
        let range = 400..480;
        let window = Flow::window(Arc::clone(&blocks), range, &typo, Fonts::bundled(), None)
            .ok_or("window layout failed")?;
        assert!(window.page_count() < full.page_count());
        let target = Anchor {
            block: 420,
            offset: 0,
        };
        let page = window.locate(target);
        assert_eq!(window.anchor(0).map(|a| a.block), Some(400));
        assert!(window.excerpts(page).iter().any(|(b, _)| *b == 420));
        assert!(!window.text(page).is_empty());
        let px = window.render(page, 0.25, [0, 0, 100, 100]);
        assert!(px.is_some());
        Ok(())
    }

    #[test]
    fn cancellable_layout_stops_early_when_flag_is_set() {
        let blocks = many_blocks(800);
        let typo = Typography::default();
        let cancel = AtomicBool::new(true);
        let out = Flow::new_cancellable(blocks, &typo, Fonts::bundled(), &cancel);
        assert!(out.is_none());
    }
}
