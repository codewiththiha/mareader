//! CSS multi-column flow (`column-count`, `width`, `gap`, `rule`).
use std::ops::Range;

use crate::anim::MultiCol;
use crate::block::Edge;

use super::{Cursor, Deco, Piece, Placed, Set, Typography, gap, place_block, resolve};

/// Resolved column geometry inside a multi-column container.
#[derive(Debug, Clone, Copy)]
pub(super) struct ColumnGeom {
    pub(super) count: usize,
    pub(super) width: f32,
    pub(super) gap: f32,
    pub(super) step: f32,
}

/// Computes column count, width, and gap for `mc` inside `room`.
pub(super) fn geometry(mc: &MultiCol, room: f32, em: f32) -> ColumnGeom {
    let gap = resolve(mc.gap, room, em).clamp(0.0, (room * 0.4).max(0.0));
    let mut count = usize::from(mc.count).min(8);
    if let Some(pref) = mc.width {
        let pref_w = resolve(pref, room, em).max(em.max(1.0));
        let fit = (((room + gap) / (pref_w + gap)).floor() as usize).clamp(1, 8);
        count = if count == 0 { fit } else { count.min(fit) };
    }
    let count = count.max(1);
    let width = ((room - gap * (count.saturating_sub(1)) as f32) / count as f32).max(em.max(1.0));
    ColumnGeom {
        count,
        width,
        gap,
        step: width + gap,
    }
}

/// Finds the innermost multi-column box enclosing `block`.
pub(super) fn box_key(block: &Set) -> Option<(usize, usize)> {
    if block.column_span() {
        return None;
    }
    block
        .boxes
        .iter()
        .enumerate()
        .rev()
        .find(|(_, b)| b.multicol.is_some())
        .map(|(depth, b)| (b.key, depth))
}

/// Flows `range` of blocks side by side across a multi-column box.
pub(super) fn place_multicol(
    c: &mut Cursor,
    blocks: &[Set],
    range: Range<usize>,
    (key, depth): (usize, usize),
    (height, typo): (f32, &Typography),
) {
    let Some(first) = blocks.get(range.start) else {
        return;
    };
    let Some(placed_box) = first.boxes.get(depth) else {
        for i in range {
            place_block(c, blocks, i, (height, typo));
        }
        return;
    };
    let Some(mc) = placed_box.multicol.as_deref() else {
        for i in range {
            place_block(c, blocks, i, (height, typo));
        }
        return;
    };
    let inside_left = placed_box.border[3] + placed_box.padding[3];
    let inside_right = placed_box.border[1] + placed_box.padding[1];
    let room = (placed_box.width - inside_left - inside_right).max(placed_box.em);
    let geom = geometry(mc, room, placed_box.em);
    if geom.count <= 1 {
        for i in range {
            place_block(c, blocks, i, (height, typo));
        }
        return;
    }
    c.settle();
    let outer = first.boxes.get(..=depth).unwrap_or(&first.boxes);
    c.close(c.common(outer));
    if first.break_before {
        c.break_page();
    }
    if first.clear {
        c.y = c.y.max(c.float_bottom);
    }
    if !c.page.is_empty() {
        c.y += gap(first, typo);
    }
    c.open(outer);
    let avail = (height - c.y).max(placed_box.em);
    let explicit = placed_box.height.map(|h| h.min(avail));
    // Measure all units in the multi-column segment to balance columns evenly.
    let (total_h, max_unit) = measure_segment(blocks, range.clone(), typo);
    let target = match explicit {
        Some(h) if total_h > h * geom.count as f32 => h,
        Some(h) => (total_h / geom.count as f32).max(max_unit).min(h),
        None => (total_h / geom.count as f32).max(max_unit).min(avail),
    };
    let origin_y = c.y;
    let (max_h, used_cols) = fill_columns(c, blocks, range, (geom, origin_y, target), typo);
    emit_rules(c, placed_box, mc, (geom, origin_y, max_h, used_cols));
    c.y = origin_y + max_h;
    let _ = key;
}

fn fill_columns(
    c: &mut Cursor,
    blocks: &[Set],
    range: Range<usize>,
    (geom, origin_y, target): (ColumnGeom, f32, f32),
    typo: &Typography,
) -> (f32, usize) {
    let (mut col, mut col_y, mut max_h) = (0usize, 0.0f32, 0.0f32);
    let mut used_cols = 0usize;
    for idx in range {
        let Some(block) = blocks.get(idx) else {
            continue;
        };
        let mut u = 0usize;
        while u < block.units.len() {
            let lead = if col_y > 0.0 && u == 0 {
                gap(block, typo)
            } else {
                0.0
            };
            let start_top = block.units.get(u).map_or(0.0, |t| t.0);
            let mut end = u;
            while end < block.units.len() {
                let bot = block.units.get(end).map_or(start_top, |t| t.1);
                if end > u && col_y + lead + (bot - start_top) > target && col + 1 < geom.count {
                    break;
                }
                end += 1;
            }
            let first_h = block
                .units
                .get(end.saturating_sub(1))
                .map_or(0.0, |t| t.1 - start_top);
            if col_y > 0.0 && col_y + lead + first_h > target && col + 1 < geom.count {
                max_h = max_h.max(col_y);
                col += 1;
                col_y = 0.0;
                continue;
            }
            let y_here = col_y + lead;
            c.page.push(Piece {
                block: idx,
                units: u..end,
                dx: col as f32 * geom.step,
                dy: origin_y + y_here - start_top,
            });
            col_y = y_here + first_h;
            max_h = max_h.max(col_y);
            used_cols = used_cols.max(col + 1);
            u = end;
            if u < block.units.len() && col + 1 < geom.count {
                col += 1;
                col_y = 0.0;
            }
        }
    }
    (max_h, used_cols)
}

fn measure_segment(blocks: &[Set], range: Range<usize>, typo: &Typography) -> (f32, f32) {
    let (mut total, mut max_u) = (0.0f32, 0.0f32);
    for (n, idx) in range.enumerate() {
        let Some(b) = blocks.get(idx) else {
            continue;
        };
        if b.units.is_empty() {
            continue;
        }
        if n > 0 {
            total += gap(b, typo);
        }
        let first_top = b.units.first().map_or(0.0, |u| u.0);
        let last_bot = b.units.last().map_or(first_top, |u| u.1);
        total += (last_bot - first_top).max(0.0);
        for &(t, bot) in &b.units {
            max_u = max_u.max((bot - t).max(0.0));
        }
    }
    (total, max_u)
}

fn emit_rules(
    c: &mut Cursor,
    box_spec: &Placed,
    mc: &MultiCol,
    (geom, top, height, used_cols): (ColumnGeom, f32, f32, usize),
) {
    let rule_w = mc.rule_width * box_spec.em;
    if used_cols < 2 || rule_w <= 0.0 || !mc.rule_style.draws() || height <= 0.5 {
        return;
    }
    let content_x = box_spec.x + box_spec.border[3] + box_spec.padding[3];
    let rgb = mc.rule_colour.map(|[r, g, b, _]| [r, g, b]);
    for col in 0..(used_cols - 1) {
        let rx = content_x + col as f32 * geom.step + geom.width + (geom.gap - rule_w) * 0.5;
        c.deco.push(Deco {
            rect: [rx, top, rule_w, height],
            border: [0.0, 0.0, 0.0, rule_w],
            edges: [Edge::None, Edge::None, Edge::None, mc.rule_style],
            colours: [None, None, None, rgb],
            fill: None,
            fill_rgba: None,
            backdrop: None,
            em: box_spec.em,
            radius: 0.0,
            finish: None,
            continued: [false, false],
            box_key: 0,
            clip: false,
            transform: None,
            interaction: None,
            z: 0,
        });
    }
}
