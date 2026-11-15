use super::*;

fn column(columns: usize, b: &Set) -> usize {
    b.cell().map_or(0, |c| c.column).min(columns - 1)
}

pub(super) struct RowMeasure<'a> {
    columns: usize,
    heights: Vec<f32>,
    last: Vec<Option<&'a Set>>,
    tops: Vec<f32>,
    tall: f32,
    pub(super) need: f32,
}

pub(super) fn measure<'a>(
    members: &'a [Set],
    typo: &Typography,
    height: f32,
    cell: CellAt,
) -> Option<RowMeasure<'a>> {
    let columns = members
        .iter()
        .filter_map(|b| b.cell())
        .map(|c| c.column + 1)
        .max();
    let columns = columns.unwrap_or(1).clamp(1, MAX_COLUMNS);
    // Each block's top in its column, then each column's height.
    let mut heights = vec![0.0; columns];
    let mut last: Vec<Option<&Set>> = vec![None; columns];
    let mut tops = Vec::with_capacity(members.len());
    // A float in a column: its key and top; then where floats end.
    let mut floats: Vec<Option<(usize, f32)>> = vec![None; columns];
    let mut float_ends = vec![0.0f32; columns];
    for b in members {
        let col = column(columns, b);
        let (Some(h), Some(end)) = (heights.get_mut(col), last.get_mut(col)) else {
            continue;
        };
        let (Some(float), Some(float_end)) = (floats.get_mut(col), float_ends.get_mut(col)) else {
            continue;
        };
        if let Some((key, start)) = *float
            && b.float().map(|f| f.key) != Some(key)
        {
            // The float ends: what follows starts at its top, beside it.
            (*float_end, *h, *float) = (float_end.max(*h), start, None);
        }
        if b.clear {
            *h = h.max(*float_end);
        }
        if let (Some(f), None) = (b.float(), *float) {
            *float = Some((f.key, *h));
        }
        let gap = match end {
            Some(_) => gap(b, typo),
            None => b.before.unwrap_or(0.0).max(0.0),
        };
        tops.push(*h + gap);
        *h += gap + span_height(b, 0..b.units.len());
        *end = Some(b);
    }
    // A column ends below its floats.
    for (h, end) in heights.iter_mut().zip(&float_ends) {
        *h = h.max(*end);
    }
    // A cell's last block closes the cell's boxes under it.
    for (h, b) in heights.iter_mut().zip(&last) {
        let inner = b.map_or(&[][..], |b| b.boxes.get(cell.depth..).unwrap_or_default());
        *h += inner.iter().map(|p| p.bottom).fold(0.0, f32::max);
    }
    // A cell spanning down leaves this row's height to the rest.
    let down = |col: usize| {
        let b = last.get(col).copied().flatten();
        b.and_then(|b| b.cell()).map_or(1, |c| c.down)
    };
    let (mut tall, mut need) = (0.0f32, 0.0f32);
    for (col, h) in heights.iter().enumerate() {
        need = need.max(*h);
        if down(col) <= 1 {
            tall = tall.max(*h);
        }
    }
    if need > height {
        return None;
    }
    Some(RowMeasure {
        columns,
        heights,
        last,
        tops,
        tall,
        need,
    })
}

pub(super) fn place(
    c: &mut Cursor,
    row: Range<usize>,
    members: &[Set],
    cell: CellAt,
    measure: RowMeasure<'_>,
) {
    let RowMeasure {
        columns,
        heights,
        last,
        tops,
        tall,
        need: _,
    } = measure;
    let down = |col: usize| {
        let b = last.get(col).copied().flatten();
        b.and_then(|b| b.cell()).map_or(1, |c| c.down)
    };
    let top = c.y;
    let mut placed = Vec::with_capacity(members.len());
    let mut hung: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for ((k, b), at) in row.clone().zip(members).zip(&tops) {
        let col = column(columns, b);
        let room = tall - heights.get(col).copied().unwrap_or(0.0);
        let shift = match b.cell().map(|c| c.valign) {
            _ if down(col) > 1 => 0.0,
            Some(VAlign::Top) | None => 0.0,
            Some(VAlign::Middle) => room / 2.0,
            Some(VAlign::Bottom) => room,
        };
        let y = top + shift + at;
        let unit = b.units.first().map_or(0.0, |u| u.0);
        if let Some(pieces) = hung.get_mut(col).filter(|_| down(col) > 1) {
            pieces.push(c.page.len());
        }
        c.page.push(Piece {
            block: k,
            units: 0..b.units.len(),
            dy: y - unit,
        });
        placed.push((col, b, y));
    }
    let mut decos = vec![0..0; columns];
    for col in 0..columns {
        let stack = placed.iter().filter(|(c, ..)| *c == col);
        let from = c.deco.len();
        cell_decos(stack.map(|(_, b, y)| (*b, *y)), cell.depth, &mut c.deco);
        if let Some(d) = decos.get_mut(col) {
            *d = from..c.deco.len();
        }
    }
    c.y = top + tall;
    c.row_done(cell.grid);
    for (col, (pieces, decos)) in hung.into_iter().zip(decos).enumerate() {
        if pieces.is_empty() {
            continue;
        }
        let valign = last.get(col).copied().flatten().and_then(|b| b.cell());
        c.hangs.push(Hang {
            grid: cell.grid,
            rows: down(col) - 1,
            bottom: top + heights.get(col).copied().unwrap_or(0.0),
            valign: valign.map_or(VAlign::Top, |c| c.valign),
            pieces,
            decos,
        });
    }
}
