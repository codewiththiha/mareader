//! Study mode in the page grid: how each locked page stands, and which
//! batch's cards the reader faces.
//!
//! A page whose cards are not ready is dimmed under a padlock; one whose
//! cards the AI writes now glows too; one whose cards are ready keeps the
//! padlock, undimmed. Pages that read freely show as they are. The batch
//! whose deck the reader faces has an accent bar beside it.

use std::f32::consts::PI;

use egui::{Color32, CornerRadius, Pos2, Rect, Shape, Stroke, Ui, pos2, vec2};
use mareader_engine::study_mode::{Stand, Stands};

use crate::theme::Tokens;

/// What the grid marks of a pane's Proactive study, as of this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StudyMarks {
    pub stands: Stands,
    /// The batch whose deck the reader faces, first and last page.
    pub facing: Option<(usize, usize)>,
}

impl StudyMarks {
    pub fn stand(&self, page: usize) -> Stand {
        let held = self.stands.iter().find(|(pages, _)| pages.contains(&page));
        held.map_or(Stand::Free, |(_, stand)| *stand)
    }

    pub fn faces(&self, page: usize) -> bool {
        self.facing.is_some_and(|(a, b)| (a..=b).contains(&page))
    }
}

/// Over a drawn page: the wash and padlock of where it stands, and the
/// accent bar beside it while the reader faces its cards.
pub fn over(ui: &Ui, page: Rect, (stand, facing): (Stand, bool), t: &Tokens) {
    let painter = ui.painter();
    if matches!(stand, Stand::Shut | Stand::Writing) {
        // Light paper on a light rail fades into it unseen, so it greys.
        let fill = match t.dark {
            true => ui.visuals().panel_fill.gamma_multiply(0.55),
            false => Color32::BLACK.gamma_multiply(0.16),
        };
        painter.rect_filled(page, CornerRadius::ZERO, fill);
    }
    if facing {
        let bar = Rect::from_min_max(pos2(page.left() - 7.0, page.top()), page.left_bottom());
        let bar = bar.shrink2(vec2(1.5, 0.0));
        painter.rect_filled(bar, CornerRadius::same(2), t.accent);
    }
    if stand != Stand::Free {
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
    fn a_page_stands_as_its_span_says_and_the_rest_read_freely() {
        let marks = StudyMarks {
            stands: vec![(3..6, Stand::Ready), (6..9, Stand::Writing)],
            facing: Some((3, 5)),
        };
        assert_eq!(marks.stand(1), Stand::Free);
        assert_eq!(marks.stand(5), Stand::Ready);
        assert_eq!(marks.stand(6), Stand::Writing);
        assert!(marks.faces(4) && !marks.faces(6));
        assert_eq!(StudyMarks::default().stand(4), Stand::Free);
    }
}
