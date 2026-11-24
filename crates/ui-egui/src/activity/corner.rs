//! The pane's bottom left corner, which the AI's rings share: its stack
//! first, then the result bubbles of the page there, in a row level with
//! the page counter across the pane.

use egui::{Context, Id, Rect};

use crate::page_bar::{BAR_H, COUNTER_MID};

/// Room between a ring and the next in the row.
const GAP: f32 = 6.0;
/// Room from the pane's left edge.
const INSET: f32 = 12.0;
/// Room above the page bar, while it is up.
const LIFT: f32 = 12.0;
/// A stack's ring's side, as `ring::SIDE`.
const SIDE: f32 = super::ring::SIDE;

/// One pane's row, as the last frame left it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    pane: Rect,
    /// The row's middle, down the screen.
    pub mid: f32,
    /// Where the row is free from, after the stack.
    pub from: f32,
}

fn id() -> Id {
    Id::new("activity-corner-rows")
}

/// The row's middle in `pane`, risen over the page bar while `bar`.
pub fn mid(pane: Rect, bar: bool) -> f32 {
    match bar {
        true => pane.bottom() - BAR_H - LIFT - SIDE / 2.0,
        false => pane.bottom() - COUNTER_MID,
    }
}

/// Sets the rows of this frame's `panes`, each with whether its bar is up.
pub fn share(ctx: &Context, panes: &[(Rect, bool)]) {
    let row = |&(pane, bar): &(Rect, bool)| Row {
        pane,
        mid: mid(pane, bar),
        from: pane.left() + INSET,
    };
    let rows: Vec<Row> = panes.iter().map(row).collect();
    ctx.data_mut(|d| d.insert_temp(id(), rows));
}

/// Says a folded stack stands at `ring`; the row in its pane goes on after.
pub fn stand(ctx: &Context, ring: Rect) {
    ctx.data_mut(|d| {
        let rows = d.get_temp_mut_or_default::<Vec<Row>>(id());
        let here = |r: &&mut Row| r.pane.contains(ring.center());
        let level = |r: &&mut Row| here(r) && (r.mid - ring.center().y).abs() < 1.0;
        if let Some(row) = rows.iter_mut().find(level) {
            row.from = row.from.max(ring.right() + GAP);
        }
    });
}

/// The row of the pane holding `seen`'s middle, as the last frame left it.
pub fn row(ctx: &Context, seen: Rect) -> Option<Row> {
    let rows: Vec<Row> = ctx.data(|d| d.get_temp(id())).unwrap_or_default();
    rows.into_iter().find(|r| r.pane.contains(seen.center()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};

    #[test]
    fn a_stack_in_the_corner_moves_the_bubbles_on() {
        let ctx = Context::default();
        let pane = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0));
        share(&ctx, &[(pane, false)]);
        let page = Rect::from_min_size(pos2(0.0, 0.0), vec2(300.0, 400.0));
        let free = row(&ctx, page).map(|r| (r.mid, r.from));
        assert_eq!(
            free,
            Some((400.0 - COUNTER_MID, INSET)),
            "level with the counter"
        );
        let at = pos2(INSET + SIDE / 2.0, 400.0 - COUNTER_MID);
        stand(&ctx, Rect::from_center_size(at, vec2(SIDE, SIDE)));
        let after = row(&ctx, page).map(|r| r.from);
        assert_eq!(after, Some(INSET + SIDE + GAP), "after the stack");
    }
}
