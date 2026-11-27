//! Space's notes: every highlight, note and bookmark, a sheet per book.
//!
//! The engine reads the marks into groups (`mareader_engine::space::notes`);
//! this draws the header, the chips, then each book's sheet. A row opens
//! its book at the mark.

use egui::text::LayoutJob;
use egui::{Align2, Color32, CursorIcon, FontId, Frame, Galley, Rect, Sense, Stroke, Ui};
use egui::{pos2, vec2};
use mareader_engine::command::Command;
use mareader_engine::space::{self, Kept, NoteGroup, NoteRow, NotesFilter, NotesView, Room};
use mareader_settings::HighlightSettings;
use std::sync::Arc;

use super::{
    look,
    note_selection::{Item, Selection},
    top,
};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;

/// The page's own label, which tests and the titlebar find it by.
pub const TITLE: &str = "Notes";
const LINE: &str = "Highlights, notes, bookmarks and saved AI answers from every book.";
const SEARCH_W: f32 = 320.0;
const NONE_YET: &str = "Highlight words while reading, or add a note or a bookmark.";
const NONE_MATCH: &str = "Try other words, another chip or another book.";
/// A row's insets, the colour rule's width, and the room the date takes.
const PAD_X: f32 = 20.0;
const PAD_Y: f32 = 14.0;
const RULE_W: f32 = 3.0;
const META_W: f32 = 96.0;
/// At most this many lines of a quote, and of a note.
const QUOTE_ROWS: usize = 3;
const NOTE_ROWS: usize = 4;

/// The header, the chips, then each book's sheet.
///
/// `narrow` puts the books in a menu under the chips, as there is no rail.
pub(super) fn column(
    ui: &mut Ui,
    (view, filter): (&NotesView, &mut NotesFilter),
    (look, narrow): (&HighlightSettings, bool),
    selection: &mut Selection,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
) {
    ui.spacing_mut().item_spacing.y = 14.0;
    if narrow && look::back(ui, t) {
        out.push(Sent::Engine(Command::ShowSpace(Room::Hub)));
    }
    header(ui, filter, t);
    let items = Kept::ALL.map(|k| (k, k.label(), view.counts[k as usize]));
    if ui.available_width() < 500.0 {
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let (first, second) = items.split_at(2);
            top::tab_row(ui, first, &mut filter.kept, t);
            if ui.available_width() < 300.0 {
                let (bookmarks, ai) = second.split_at(1);
                top::tab_row(ui, bookmarks, &mut filter.kept, t);
                top::tab_row(ui, ai, &mut filter.kept, t);
            } else {
                top::tab_row(ui, second, &mut filter.kept, t);
            }
        });
    } else {
        top::tab_row(ui, &items, &mut filter.kept, t);
    }
    if narrow {
        books(ui, view, filter);
    }
    ui.add_space(4.0);
    if view.shown == 0 {
        let (title, hint) = if filter.kept == Kept::Ai && view.counts[Kept::Ai as usize] == 0 {
            (
                "No AI selection results yet",
                "Save an AI answer on a selection while reading.",
            )
        } else if filter.kept == Kept::Ai {
            ("No AI results match", NONE_MATCH)
        } else if view.total == 0 {
            ("No notes yet", NONE_YET)
        } else {
            ("No notes match", NONE_MATCH)
        };
        widgets::panel_empty(ui, Some(Icon::Note), title, hint);
        return;
    }
    selection.toolbar(ui, t);
    selection.begin();
    ui.spacing_mut().item_spacing.y = 10.0;
    for group in &view.groups {
        heading(ui, group, t);
        sheet(ui, group, look, selection, reveal, t, out);
        ui.add_space(8.0);
    }
    super::ai_notes::show(ui, &view.ai_groups, selection, reveal, t, out);
    selection.finish();
}

/// On narrow screens, stack the search below the full-width heading.
fn header(ui: &mut Ui, filter: &mut NotesFilter, t: &Tokens) {
    if ui.available_width() < 560.0 {
        header_words(ui, t);
        search(ui, filter);
    } else {
        ui.horizontal_top(|ui| {
            let wide = ui.available_width();
            let words = (wide - SEARCH_W - 24.0).max(wide * 0.5);
            ui.allocate_ui_with_layout(
                vec2(words, 0.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| header_words(ui, t),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                search(ui, filter)
            });
        });
    }
}

fn header_words(ui: &mut Ui, t: &Tokens) {
    ui.spacing_mut().item_spacing.y = 4.0;
    let title = egui::RichText::new(TITLE).font(theme::semibold(30.0));
    ui.add(egui::Label::new(title.color(t.ink)).selectable(false));
    let line = egui::RichText::new(LINE).size(t.text + 1.0).color(t.muted);
    ui.add(egui::Label::new(line).wrap().selectable(false));
}

fn search(ui: &mut Ui, filter: &mut NotesFilter) {
    let width = SEARCH_W.min(ui.available_width());
    widgets::input(&mut filter.text)
        .id(egui::Id::new("space-notes-search"))
        .icon(Icon::Search)
        .hint("Search quotes, notes and AI answers")
        .label("Search your notes")
        .clearable(true)
        .width(width)
        .show(ui);
}

/// The books as a menu, for a page too narrow for the rail.
fn books(ui: &mut Ui, view: &NotesView, filter: &mut NotesFilter) {
    let picked = view.books.iter().find(|s| Some(s.id) == filter.book);
    let shown = picked.map_or("All books", |s| s.title.as_str());
    let mut items = vec![(None, "All books".to_owned())];
    items.extend(view.books.iter().map(|s| (Some(s.id), s.title.clone())));
    let width = ui.available_width().min(320.0);
    crate::dropdown::select(
        ui,
        egui::Id::new("space-notes-book"),
        shown,
        width,
        &mut filter.book,
        &items,
    );
}

/// A book's title and its count in a soft pill.
fn heading(ui: &mut Ui, group: &NoteGroup, t: &Tokens) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let name = egui::RichText::new(&group.title).font(theme::semibold(t.text + 2.0));
        ui.add(egui::Label::new(name.color(t.ink)).selectable(false));
        let n = group.rows.len().to_string();
        look::tag(ui, &n, (t.muted, t.ink.gamma_multiply(0.07)), 10.0);
    });
}

/// A book's rows on one raised sheet, a hairline between each.
fn sheet(
    ui: &mut Ui,
    group: &NoteGroup,
    look: &HighlightSettings,
    selection: &mut Selection,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
) {
    let edge = Stroke::new(1.0, t.line);
    let frame = Frame::new()
        .fill(t.card)
        .stroke(edge)
        .corner_radius(look::ROUND);
    frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let last = group.rows.len().saturating_sub(1);
        for (i, row) in group.rows.iter().enumerate() {
            let ends = (i == 0, i == last);
            if line(ui, row, (look, ends), selection, reveal, t, out) {
                out.push(Sent::Ui(UiCommand::OpenSpaceNote {
                    book: row.book,
                    mark: row.id,
                }));
            }
            if i < last {
                let rule = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
                let x = rule.0.x_range().shrink(PAD_X);
                ui.painter().hline(x, rule.0.center().y, edge);
            }
        }
    });
}

/// One mark: its colour's rule, the words, the note, then where and when;
/// true when clicked. `ends` says it is the sheet's first or last row.
fn line(
    ui: &mut Ui,
    row: &NoteRow,
    (look, ends): (&HighlightSettings, (bool, bool)),
    selection: &mut Selection,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
) -> bool {
    let wide = ui.available_width();
    let text_x = PAD_X + RULE_W + 14.0;
    let text_w = (wide - text_x - META_W - PAD_X).max(80.0);
    let quote = quote(ui, row, text_w, t);
    let note = row.note.as_deref().map(|n| note(ui, n, text_w - 22.0, t));
    let note_h = note.as_ref().map_or(0.0, |g| g.size().y + 10.0);
    let tall = (PAD_Y * 2.0 + quote.size().y + note_h).max(56.0);
    let (rect, r) = ui.allocate_exact_size(vec2(wide, tall), Sense::click());
    if r.hovered() || selection.selected(Item::Mark(row.id)) {
        let round = egui::CornerRadius {
            nw: if ends.0 { look::ROUND as u8 } else { 0 },
            ne: if ends.0 { look::ROUND as u8 } else { 0 },
            sw: if ends.1 { look::ROUND as u8 } else { 0 },
            se: if ends.1 { look::ROUND as u8 } else { 0 },
        };
        ui.painter().rect_filled(
            rect.shrink(1.0),
            round,
            if selection.selected(Item::Mark(row.id)) {
                t.accent_soft
            } else {
                t.hover
            },
        );
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    let ink = ink(row, look, t);
    let rule = Rect::from_min_size(
        rect.min + vec2(PAD_X, PAD_Y),
        vec2(RULE_W, tall - 2.0 * PAD_Y),
    );
    painter.rect_filled(rule, 2.0, ink);
    let mut y = rect.top() + PAD_Y;
    let quote_h = quote.size().y;
    painter.galley(pos2(rect.left() + text_x, y), quote, t.ink);
    y += quote_h + 10.0;
    if let Some(note) = note {
        let mark = Rect::from_min_size(pos2(rect.left() + text_x, y + 1.0), vec2(14.0, 14.0));
        Icon::Note.paint(ui, mark, t.muted);
        ui.painter()
            .galley(pos2(mark.right() + 8.0, y), note, t.ink);
    }
    meta(ui, rect, row, t);
    let label = format!("Open {}", row.quote);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    if rect.intersect(ui.clip_rect()).is_positive() {
        selection.visible(Item::Mark(row.id));
    }
    if let Some((key, pending, _)) = reveal
        && *key == row.id.0
        && *pending
    {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
        *pending = false;
    }
    super::locateglow::draw(ui, rect, row.id.0, reveal, t);
    selection.menu(&r, Item::Mark(row.id), row.book, out);
    if r.clicked() && selection.active() {
        selection.toggle(Item::Mark(row.id));
        false
    } else {
        r.clicked()
    }
}

/// The mark's colour as the reader paints it; a bookmark takes the accent.
fn ink(row: &NoteRow, look: &HighlightSettings, t: &Tokens) -> Color32 {
    match row.bookmark {
        true => t.accent,
        false => theme::color(crate::study::mark_color(row.style, look)),
    }
}

/// The marked words in quotes, or a bookmark's name.
fn quote(ui: &Ui, row: &NoteRow, width: f32, t: &Tokens) -> Arc<Galley> {
    let (text, font) = match row.bookmark {
        true => (row.quote.clone(), theme::medium(t.text + 1.0)),
        false => (
            format!("“{}”", row.quote),
            FontId::proportional(t.text + 1.5),
        ),
    };
    wrapped(ui, text, font, t.ink, (width, QUOTE_ROWS))
}

/// A note in the reader's ink, a little smaller than the words it is on.
fn note(ui: &Ui, text: &str, width: f32, t: &Tokens) -> Arc<Galley> {
    let font = FontId::proportional(t.text);
    wrapped(
        ui,
        text.to_owned(),
        font,
        t.ink.gamma_multiply(0.82),
        (width, NOTE_ROWS),
    )
}

fn wrapped(
    ui: &Ui,
    text: String,
    font: FontId,
    ink: Color32,
    (w, rows): (f32, usize),
) -> Arc<Galley> {
    let mut job = LayoutJob::simple(text, font, ink, w);
    job.wrap.max_rows = rows;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

/// The page, when known, over the day it was made, at the row's right.
fn meta(ui: &Ui, rect: Rect, row: &NoteRow, t: &Tokens) {
    let painter = ui.painter();
    let right = rect.right() - PAD_X;
    let mut y = rect.top() + PAD_Y + 1.0;
    let small = FontId::proportional(t.text_small);
    if row.bookmark {
        let tag = theme::medium(t.text_small - 1.0);
        let r = painter.text(pos2(right, y), Align2::RIGHT_TOP, "BOOKMARK", tag, t.accent);
        y = r.bottom() + 4.0;
    }
    if let Some(page) = row.page {
        let words = format!("Page {}", page + 1);
        let r = painter.text(
            pos2(right, y),
            Align2::RIGHT_TOP,
            words,
            theme::medium(t.text_small),
            t.ink,
        );
        y = r.bottom() + 3.0;
    }
    painter.text(
        pos2(right, y),
        Align2::RIGHT_TOP,
        space::day(row.added),
        small,
        t.muted,
    );
}
