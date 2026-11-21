//! The rail down Space's left side: the books, levels, learner and word
//! class that narrow the rows.

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CursorIcon, FontId, Rect, Sense, Ui, WidgetInfo, WidgetType, vec2};
use mareader_engine::space::{BANDS, Filter, View};
use mareader_library::{BookId, LearnedBy};

use super::look;
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

const CARDS: Option<LearnedBy> = Some(LearnedBy::Proactive);
const RECALL: Option<LearnedBy> = Some(LearnedBy::Reactive);
const LEARNERS: [Option<LearnedBy>; 3] = [None, CARDS, RECALL];
const ROW_H: f32 = 40.0;
/// A level chip's size.
const CHIP: egui::Vec2 = vec2(46.0, 32.0);
/// A book's title is cut to this many letters in the rail.
const TITLE_CHARS: usize = 22;

/// Draws the rail; each choice narrows `filter`.
pub(super) fn show(ui: &mut Ui, view: &View, filter: &mut Filter, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 10.0;
    look::caption(ui, "Books", t);
    books(ui, view, &mut filter.book, t);
    ui.add_space(14.0);
    look::caption(ui, "Level", t);
    ui.horizontal_wrapped(|ui| levels(ui, &mut filter.levels, t));
    ui.add_space(14.0);
    look::caption(ui, "Learned by", t);
    let by = &mut filter.by;
    widgets::segments_of(ui, 34.0, by, &LEARNERS, 3, |by| (None, learner(by)));
    ui.add_space(14.0);
    look::caption(ui, "Part of speech", t);
    class(ui, &view.poses, &mut filter.pos);
}

/// All books, then each book with a word in it.
fn books(ui: &mut Ui, view: &View, book: &mut Option<BookId>, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 2.0;
    let all = Shelf {
        title: "All books",
        kind: "",
        count: view.total,
    };
    if shelf(ui, book.is_none(), all, t) {
        *book = None;
    }
    for s in &view.books {
        let title = cut(&s.title);
        let row = Shelf {
            title: &title,
            kind: s.format.label(),
            count: s.count,
        };
        if shelf(ui, *book == Some(s.id), row, t) {
            *book = Some(s.id);
        }
    }
    ui.spacing_mut().item_spacing.y = 10.0;
}

/// One rail row's words: a title, its format, and its words counted.
struct Shelf<'a> {
    title: &'a str,
    kind: &'a str,
    count: usize,
}

/// One book's row; true when clicked.
fn shelf(ui: &mut Ui, on: bool, row: Shelf<'_>, t: &Tokens) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let fill = match (on, r.hovered()) {
        (true, _) => t.selected,
        (false, true) => t.hover,
        (false, false) => Color32::TRANSPARENT,
    };
    ui.painter().rect_filled(rect, 12.0, fill);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let (left, right) = (rect.left_center(), rect.right_center());
    let mut x = left.x + 14.0;
    if !row.kind.is_empty() {
        let mark = Rect::from_center_size(egui::pos2(x + 8.0, left.y), vec2(16.0, 16.0));
        Icon::Book.paint(ui, mark, t.muted);
        x += 26.0;
    }
    let font = match on {
        true => theme::medium(t.text + 1.0),
        false => FontId::proportional(t.text + 1.0),
    };
    let painter = ui.painter();
    let small = FontId::proportional(t.text_small);
    let count = right - vec2(14.0, 0.0);
    let n = row.count.to_string();
    let drawn = painter.text(count, Align2::RIGHT_CENTER, n, small, t.muted);
    let mut end = drawn.left();
    if !row.kind.is_empty() {
        let at = egui::pos2(end - 14.0, right.y);
        let font = theme::medium(10.5);
        let drawn = painter.text(at, Align2::RIGHT_CENTER, row.kind, font, t.muted);
        end = drawn.left();
    }
    let galley = painter.layout_job(fitted(row.title, font, t.ink, end - 10.0 - x));
    let top = egui::pos2(x, left.y - galley.size().y / 2.0);
    painter.galley(top, galley, t.ink);
    let title = row.title;
    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, title));
    r.clicked()
}

/// `text` on one line `width` wide at most, cut short with an ellipsis.
fn fitted(text: &str, font: FontId, color: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping {
        max_width: width.max(0.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    job
}

/// `title`, cut to fit the rail.
fn cut(title: &str) -> String {
    match title.char_indices().nth(TITLE_CHARS) {
        Some((at, _)) => format!("{}…", title.get(..at).unwrap_or(title).trim_end()),
        None => title.to_owned(),
    }
}

/// B1 to C2 as chips; none picked shows every level.
fn levels(ui: &mut Ui, levels: &mut Vec<u8>, t: &Tokens) {
    ui.spacing_mut().item_spacing.x = 8.0;
    for band in BANDS {
        let on = levels.contains(&band);
        if chip(ui, on, mareader_cefr::label(band), t) {
            match on {
                true => levels.retain(|b| *b != band),
                false => levels.push(band),
            }
        }
    }
}

/// A square chip that picks or drops a level; true when clicked.
fn chip(ui: &mut Ui, on: bool, text: &str, t: &Tokens) -> bool {
    let (rect, r) = ui.allocate_exact_size(CHIP, Sense::click());
    let fill = match (on, r.hovered()) {
        (true, _) => t.selected,
        (false, true) => t.hover,
        (false, false) => Color32::TRANSPARENT,
    };
    let edge = match on {
        true => t.accent.gamma_multiply(0.5),
        false => t.line,
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 9.0, fill);
    let stroke = egui::Stroke::new(1.0, edge);
    painter.rect_stroke(rect, 9.0, stroke, egui::StrokeKind::Inside);
    let ink = if on { t.accent } else { t.ink };
    let font = theme::medium(t.text);
    painter.text(rect.center(), Align2::CENTER_CENTER, text, font, ink);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    r.widget_info(|| WidgetInfo::selected(WidgetType::Button, true, on, text));
    r.clicked()
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
        .height(240.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(pos, None, "Any");
            for p in poses {
                ui.selectable_value(pos, Some(p.clone()), p.as_str());
            }
        });
}
