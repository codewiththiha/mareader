//! Study mode in the page grid: the batch read stays bright with an accent
//! bar, the pages around it dim, and the pages past the gate wear a padlock.

use std::f32::consts::PI;

use egui::{Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};

use crate::theme::Tokens;

/// What the grid marks of a pane's Proactive study, as of this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StudyMarks {
    pub on: bool,
    /// The batch the reader is in, first and last page.
    pub batch: Option<(usize, usize)>,
    /// The page study mode turned on at.
    pub start: usize,
    /// The pages before the start are locked.
    pub lock: bool,
    /// The first page past the gate.
    pub gate: Option<usize>,
}

/// How a page shows in the grid: its wash, and whether it is locked or in
/// the batch read.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shade {
    pub wash: f32,
    pub locked: bool,
    pub batch: bool,
}

impl StudyMarks {
    pub fn shade(self, page: usize) -> Shade {
        let batch = self.on && self.batch.is_some_and(|(a, b)| (a..=b).contains(&page));
        let locked = self.on && self.gate.is_some_and(|gate| page >= gate);
        let wash = match (self.on, locked, batch) {
            (false, ..) | (true, _, true) => 0.0,
            (true, true, false) => 0.55,
            _ if page < self.start && self.lock => 0.6,
            _ => 0.35,
        };
        Shade {
            wash,
            locked,
            batch,
        }
    }
}

/// Over a drawn page: its wash and padlock, and the batch's bar beside it.
pub fn over(ui: &Ui, page: Rect, shade: Shade, t: &Tokens) {
    let painter = ui.painter();
    if shade.wash > 0.0 {
        // Light paper on a light rail fades into it unseen, so it greys.
        let fill = match t.dark {
            true => ui.visuals().panel_fill.gamma_multiply(shade.wash),
            false => Color32::BLACK.gamma_multiply(shade.wash * 0.3),
        };
        painter.rect_filled(page, CornerRadius::ZERO, fill);
    }
    if shade.batch {
        let bar = Rect::from_min_max(pos2(page.left() - 7.0, page.top()), page.left_bottom());
        let bar = bar.shrink2(vec2(1.5, 0.0));
        painter.rect_filled(bar, CornerRadius::same(2), t.accent);
    }
    if shade.locked {
        padlock(ui, page.center(), t);
    }
}

/// A small padlock drawn in place, as no icon has one.
fn padlock(ui: &Ui, centre: Pos2, t: &Tokens) {
    let painter = ui.painter();
    let body = Rect::from_center_size(centre + vec2(0.0, 3.0), vec2(14.0, 10.0));
    painter.circle_filled(centre, 13.0, t.surface.gamma_multiply(0.9));
    painter.rect_filled(body, CornerRadius::same(2), t.muted);
    let top = body.top();
    let arc = (0..=12u8).map(|i| {
        let a = PI * f32::from(i) / 12.0;
        pos2(centre.x - 4.0 * a.cos(), top - 4.5 * a.sin())
    });
    painter.add(Shape::line(arc.collect(), Stroke::new(1.6, t.muted)));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_batch_read_is_bright_and_the_pages_past_the_gate_locked() {
        let marks = StudyMarks {
            on: true,
            batch: Some((3, 7)),
            start: 3,
            lock: true,
            gate: Some(8),
        };
        let shade = |page| marks.shade(page);
        assert!(shade(4).batch && shade(4).wash < f32::EPSILON);
        assert!(shade(8).locked && !shade(8).batch);
        let (before, past) = (shade(1).wash, shade(8).wash);
        assert!(before > past, "a lock dims the pages before more");
        assert!(!StudyMarks::default().shade(1).locked);
    }
}
