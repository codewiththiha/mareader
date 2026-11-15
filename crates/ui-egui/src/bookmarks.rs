//! Bookmarks: pages kept to come back to, shown as ribbons.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use mareader_engine::session::Session;
use mareader_library::MarkId;
use mareader_workspace::PaneId;

use crate::study::StudyUi;
use mareader_engine::notes_panel::page_of;

/// The ribbon's size, hung from a page's top edge.
const RIBBON: egui::Vec2 = vec2(14.0, 22.0);

/// The pane's bookmarks, each with the page it falls on now.
pub fn on_pages(session: &Session, study: &StudyUi, pane: PaneId) -> Vec<(MarkId, usize)> {
    let Some(doc) = session.documents().document_of(pane) else {
        return Vec::new();
    };
    let book = session.book_in(pane);
    let name = session.documents().title(pane).unwrap_or_default();
    let kept = study.bookmarks_of(session.study(), session.library_model(), book, name);
    kept.iter()
        .filter_map(|&(id, spot)| Some((id, page_of(&doc, spot)?)))
        .collect()
}

/// Hangs a ribbon from `sheet`'s top edge, near its right corner.
pub fn ribbon(painter: &Painter, sheet: Rect, fill: Color32) {
    let right = sheet.right() - 18.0;
    let top = sheet.top();
    let (w, h) = (RIBBON.x, RIBBON.y);
    let at = |x: f32, y: f32| -> Pos2 { pos2(right - w + x, top + y) };
    // A notch cut into its tail, as a ribbon is.
    let points = vec![
        at(0.0, 0.0),
        at(w, 0.0),
        at(w, h),
        at(w / 2.0, h - 5.0),
        at(0.0, h),
    ];
    painter.add(Shape::convex_polygon(points, fill, Stroke::NONE));
}
