//! The rail beside Space's rows: scope, books, levels, learner and class.

use egui::{Align2, Color32, CursorIcon, FontId, Sense, Ui, WidgetInfo, WidgetType, vec2};
use mareader_engine::space::{BANDS, Filter, Scope, View};
use mareader_library::{BookId, LearnedBy};

use super::look;
use crate::theme::Tokens;
use crate::widgets;

const CARDS: Option<LearnedBy> = Some(LearnedBy::Proactive);
const RECALL: Option<LearnedBy> = Some(LearnedBy::Reactive);
const LEARNERS: [Option<LearnedBy>; 3] = [None, CARDS, RECALL];
const ROW_H: f32 = 28.0;
/// A book's title is cut to this many letters in the rail.
const TITLE_CHARS: usize = 24;

/// Draws the rail; each choice narrows `filter`.
pub(super) fn show(ui: &mut Ui, view: &View, filter: &mut Filter, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 8.0;
    look::caption(ui, "Scope", t);
    widgets::segments(ui, &mut filter.scope, &Scope::ALL, 2, |s| (None, s.label()));
    ui.add_space(6.0);
    look::caption(ui, "Books", t);
    books(ui, view, &mut filter.book, t);
    ui.add_space(6.0);
    look::caption(ui, "Level", t);
    ui.horizontal_wrapped(|ui| levels(ui, &mut filter.levels, t));
    ui.add_space(6.0);
    look::caption(ui, "Learned by", t);
    widgets::segments(ui, &mut filter.by, &LEARNERS, 3, |by| (None, learner(by)));
    ui.add_space(6.0);
    look::caption(ui, "Part of speech", t);
    class(ui, &view.poses, &mut filter.pos);
}

/// All books, then each book with a word in it.
fn books(ui: &mut Ui, view: &View, book: &mut Option<BookId>, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 0.0;
    if shelf(ui, book.is_none(), "All books", view.total, t) {
        *book = None;
    }
    for s in &view.books {
        let title = cut(&s.title);
        if shelf(ui, *book == Some(s.id), &title, s.count, t) {
            *book = Some(s.id);
        }
    }
    ui.spacing_mut().item_spacing.y = 8.0;
}

/// One book's row, its words counted at the right; true when clicked.
fn shelf(ui: &mut Ui, on: bool, title: &str, count: usize, t: &Tokens) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let fill = match (on, r.hovered()) {
        (true, _) => t.selected,
        (false, true) => t.hover,
        (false, false) => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, t.radius - 2.0, fill);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = if on { t.accent } else { t.ink };
    let font = FontId::proportional(t.text);
    let (left, right) = (rect.left_center(), rect.right_center());
    let painter = ui.painter();
    painter.text(left + vec2(8.0, 0.0), Align2::LEFT_CENTER, title, font, ink);
    let small = FontId::proportional(t.text_small);
    let at = right - vec2(8.0, 0.0);
    painter.text(at, Align2::RIGHT_CENTER, count.to_string(), small, t.muted);
    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, title));
    r.clicked()
}

/// `title`, cut to fit the rail.
fn cut(title: &str) -> String {
    match title.char_indices().nth(TITLE_CHARS) {
        Some((at, _)) => format!("{}…", title.get(..at).unwrap_or(title).trim_end()),
        None => title.to_owned(),
    }
}

/// B1 to C2 as pills; none picked shows every level.
fn levels(ui: &mut Ui, levels: &mut Vec<u8>, t: &Tokens) {
    for band in BANDS {
        let on = levels.contains(&band);
        if look::toggle(ui, on, mareader_cefr::label(band), t) {
            match on {
                true => levels.retain(|b| *b != band),
                false => levels.push(band),
            }
        }
    }
}

fn learner(by: Option<LearnedBy>) -> &'static str {
    match by {
        None => "Any",
        Some(LearnedBy::Proactive) => "Cards",
        Some(LearnedBy::Reactive) => "Recall",
    }
}

/// The word classes met, or any.
fn class(ui: &mut Ui, poses: &[String], pos: &mut Option<String>) {
    let shown = pos.as_deref().unwrap_or("Any");
    widgets::combo("space-class")
        .selected_text(shown)
        .width(ui.available_width())
        .show_ui(ui, |ui| {
            ui.selectable_value(pos, None, "Any");
            for p in poses {
                ui.selectable_value(pos, Some(p.clone()), p.as_str());
            }
        });
}
