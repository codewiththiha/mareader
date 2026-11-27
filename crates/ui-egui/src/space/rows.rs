//! Space's rows under their headings: a list on one sheet, or cards in a
//! grid.

use egui::{Align, Frame, Label, Layout, Margin, RichText, Sense, Stroke, Ui, vec2};
use mareader_engine::command::Command;
use mareader_engine::space::{self, Action, Row, Section, View};
use mareader_library::{LearnedBy, WordStatus};

mod interact;

use super::{look, selection::Selection, sentence};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use interact::{pick, row_hit};

const CARD_W: f32 = 236.0;
/// A card's margins and stroke on both sides, around its `CARD_W`.
const CARD_EDGES: f32 = 34.0;
const GAP: f32 = 12.0;
/// The chevron's room at a row's left, which its second line keeps too.
const LEAD: f32 = 30.0;
const NONE_YET: &str = "Study some pages, and the words you meet land here.";
const BOOK_HERE: &str = "Open the book here";
const NONE_MATCH: &str = "Clear a filter, or study some pages to add more.";

/// How the rows are drawn, and which one is open to its sentence.
pub(super) struct Shown<'a> {
    pub cards: bool,
    pub columns: Option<u8>,
    pub open: &'a mut Option<u128>,
    pub picked: &'a mut Option<(Row, egui::Rect)>,
    pub selection: &'a mut Selection,
    pub scope: space::Scope,
    pub reveal: &'a mut Option<(u128, bool, f64)>,
}

/// Every section, under its heading; an empty page says why.
pub(super) fn show(ui: &mut Ui, view: &View, shown: Shown<'_>, t: &Tokens, out: &mut Commands) {
    if view.rows().next().is_none() {
        let (title, hint) = match view.total {
            0 => ("No words yet", NONE_YET),
            _ => ("No words match", NONE_MATCH),
        };
        widgets::panel_empty(ui, Some(Icon::Levels), title, hint);
        return;
    }
    let Shown {
        cards,
        columns,
        open,
        picked,
        selection,
        scope,
        reveal,
    } = shown;
    selection.begin();
    ui.spacing_mut().item_spacing.y = 10.0;
    for section in &view.sections {
        heading(ui, section, t);
        match cards {
            true => grid(ui, &section.rows, picked, selection, scope, reveal, t, out, columns),
            false => sheet(
                ui,
                &section.rows,
                open,
                picked,
                selection,
                scope,
                reveal,
                t,
                out,
            ),
        }
        ui.add_space(8.0);
    }
    selection.finish();
}

/// A section's name and its count in a soft pill.
fn heading(ui: &mut Ui, section: &Section, t: &Tokens) {
    if section.label.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        let name = RichText::new(&section.label).font(theme::semibold(t.text + 2.0));
        ui.add(Label::new(name.color(t.ink)).selectable(false));
        let n = section.rows.len().to_string();
        look::tag(ui, &n, (t.muted, t.ink.gamma_multiply(0.07)), 10.0);
    });
}

/// The rows on one raised sheet, a hairline between each.
#[allow(clippy::too_many_arguments)] // Rendering carries the shared selection and return state.
fn sheet(
    ui: &mut Ui,
    rows: &[Row],
    open: &mut Option<u128>,
    picked: &mut Option<(Row, egui::Rect)>,
    selection: &mut Selection,
    scope: space::Scope,
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
        for (i, row) in rows.iter().enumerate() {
            if i > 0 {
                let size = vec2(ui.available_width(), 1.0);
                let (rule, _) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().hline(rule.x_range(), rule.center().y, edge);
            }
            ui.push_id(row.key, |ui| {
                line(ui, row, open, picked, selection, scope, reveal, t, out)
            });
        }
    });
}

/// One row: the word, its meaning, its date and badges; then its actions.
#[allow(clippy::too_many_arguments)] // Rendering carries the shared selection and return state.
fn line(
    ui: &mut Ui,
    row: &Row,
    open: &mut Option<u128>,
    picked: &mut Option<(Row, egui::Rect)>,
    selection: &mut Selection,
    scope: space::Scope,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
) {
    let is_open = *open == Some(row.key);
    let hit = row_hit(ui, row, vec2(ui.available_width(), 84.0));
    let frame = Frame::new()
        .fill(
            if selection.is_selected(row) {
                t.accent_soft
            } else if hit.hovered() {
                t.hover
            } else {
                egui::Color32::TRANSPARENT
            },
        )
        .inner_margin(Margin::symmetric(18, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 4.0;
            let word_w = (ui.available_width() * 0.24).clamp(170.0, 250.0);
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    badges(ui, row, t);
                    look::quiet(ui, &space::day(row.seen), t.text_small + 1.0, t);
                    ui.add_space(8.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if chevron(ui, is_open, &row.lemma) {
                            *open = if is_open { None } else { Some(row.key) };
                        }
                        let size = vec2(word_w, 30.0);
                        let lead = Layout::left_to_right(Align::Center);
                        let took = ui.allocate_ui_with_layout(size, lead, |ui| word(ui, row, t));
                        ui.add_space((word_w - took.response.rect.width()).max(0.0));
                        let meaning = RichText::new(&row.meaning).size(t.text + 1.0);
                        ui.add(
                            Label::new(meaning.color(t.ink))
                                .truncate()
                                .selectable(false),
                        );
                    });
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(LEAD - ui.spacing().item_spacing.x);
                actions(ui, row, out, t, selection, scope);
                if row.books.len() > 1 {
                    books(ui, row, t, out);
                }
            });
            if is_open {
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(LEAD - ui.spacing().item_spacing.x);
                    ui.vertical(|ui| sentence(ui, row, t, out));
                });
            }
        });
    super::locateglow::draw(ui, frame.response.rect, row.key, reveal, t);
    pick(
        ui,
        row,
        frame.response.rect,
        hit,
        picked,
        selection,
        scope,
        reveal,
        out,
    );
}

pub(super) fn auto_columns(width: f32) -> u8 {
    (((width + GAP) / (CARD_W + CARD_EDGES + GAP)).floor() as u8).clamp(1, 6)
}

/// The words as cards, as many to a line as the page has room for.
///
/// A framed card is a child area, which a wrapping layout cannot move down.
#[allow(clippy::too_many_arguments)] // Rendering carries the shared selection and return state.
fn grid(
    ui: &mut Ui,
    rows: &[Row],
    picked: &mut Option<(Row, egui::Rect)>,
    selection: &mut Selection,
    scope: space::Scope,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
    columns: Option<u8>,
) {
    let width = ui.available_width();
    let max_fit = ((width + GAP) / (180.0 + GAP)).floor().max(1.0) as u8;
    let across = usize::from(columns.unwrap_or(auto_columns(width)).clamp(1, max_fit));
    let card_width = ((width - GAP * (across - 1) as f32) / across as f32 - CARD_EDGES).max(1.0);
    for line in rows.chunks(across) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for row in line {
                ui.push_id(row.key, |ui| {
                    card(ui, row, picked, selection, scope, reveal, t, out, card_width)
                });
            }
        });
    }
}

#[allow(clippy::too_many_arguments)] // Rendering carries the shared selection and return state.
fn card(
    ui: &mut Ui,
    row: &Row,
    picked: &mut Option<(Row, egui::Rect)>,
    selection: &mut Selection,
    scope: space::Scope,
    reveal: &mut Option<(u128, bool, f64)>,
    t: &Tokens,
    out: &mut Commands,
    card_width: f32,
) {
    let hit = row_hit(ui, row, vec2(card_width + CARD_EDGES, 180.0));
    let frame = Frame::new()
        .fill(
            if selection.is_selected(row) {
                t.accent_soft
            } else if hit.hovered() {
                t.hover
            } else {
                t.card
            },
        )
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(look::ROUND)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(card_width);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.horizontal(|ui| word(ui, row, t));
                let meaning = RichText::new(&row.meaning).size(t.text + 1.0);
                ui.add(Label::new(meaning.color(t.ink)).wrap().selectable(false));
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    status(ui, row, t);
                    via(ui, row, t);
                    if row.first_try {
                        look::tag(ui, "first try", look::swatch(t.toast.info), 6.0);
                    }
                });
                ui.horizontal(|ui| {
                    look::quiet(ui, &space::day(row.seen), t.text_small, t);
                    let right = Layout::right_to_left(Align::Center);
                    ui.with_layout(right, |ui| forget(ui, row, selection, scope));
                });
                ui.horizontal(|ui| memorize(ui, row, out, t));
            });
        });
    super::locateglow::draw(ui, frame.response.rect, row.key, reveal, t);
    pick(
        ui,
        row,
        frame.response.rect,
        hit,
        picked,
        selection,
        scope,
        reveal,
        out,
    );
}

/// The lemma, its class in italics, and its level.
fn word(ui: &mut Ui, row: &Row, t: &Tokens) {
    ui.spacing_mut().item_spacing.x = 8.0;
    let lemma = RichText::new(&row.lemma).font(theme::medium(t.text + 5.0));
    ui.add(Label::new(lemma.color(t.ink)).selectable(false));
    if let Some(pos) = &row.pos {
        let class = RichText::new(pos).size(t.text + 1.0).italics();
        ui.add(Label::new(class.color(t.muted)).selectable(false));
    }
    look::level(ui, row.band, t);
}

/// From the right: a first try, how it was learned, then its status.
fn badges(ui: &mut Ui, row: &Row, t: &Tokens) {
    if row.first_try {
        look::tag(ui, "first try", look::swatch(t.toast.info), 6.0);
    }
    via(ui, row, t);
    status(ui, row, t);
}

fn status(ui: &mut Ui, row: &Row, t: &Tokens) {
    let colors = look::status(row.status, t);
    look::tag(ui, space::status_label(row.status), colors, 6.0);
}

/// A card's mark and "card", or the tries a recall took.
fn via(ui: &mut Ui, row: &Row, t: &Tokens) {
    let said = match (row.by, row.tries) {
        (LearnedBy::Proactive, _) => "card".to_owned(),
        (LearnedBy::Reactive, 1) => "1 try".to_owned(),
        (LearnedBy::Reactive, n) => format!("{n} tries"),
    };
    let font = egui::FontId::proportional(t.text_small);
    let galley = ui.painter().layout_no_wrap(said, font, t.muted);
    let size = vec2(14.0 + 5.0 + galley.size().x, 20.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let middle = rect.left_center() + vec2(7.0, 0.0);
    let mark = egui::Rect::from_center_size(middle, vec2(13.0, 10.0));
    let stroke = Stroke::new(1.2, t.muted);
    let inside = egui::StrokeKind::Inside;
    let painter = ui.painter();
    match row.by {
        LearnedBy::Proactive => {
            let back = mark.translate(vec2(2.0, -2.0));
            let faint = Stroke::new(1.0, t.muted.gamma_multiply(0.6));
            painter.rect_stroke(back, 2.0, faint, inside);
            painter.rect_filled(mark, 2.0, t.card);
            painter.rect_stroke(mark, 2.0, stroke, inside);
        }
        LearnedBy::Reactive => {
            painter.circle_stroke(mark.center(), 5.0, stroke);
            painter.circle_filled(mark.center(), 1.6, t.muted);
        }
    }
    let at = egui::pos2(mark.right() + 5.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, t.muted);
}

/// The books that met the word, each opening its book.
fn books(ui: &mut Ui, row: &Row, t: &Tokens, out: &mut Commands) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, (title, id)) in row.books.iter().zip(&row.book_ids).enumerate() {
        if i > 0 {
            look::quiet(ui, "·", t.text_small, t);
        }
        let words = RichText::new(title).size(t.text_small).color(t.muted);
        if look::link(ui, words, false, &open_book(title)).clicked() {
            let quote = row
                .met_at
                .as_ref()
                .filter(|at| at.book == *id)
                .map_or_else(|| row.form.clone(), |at| at.quote.clone());
            out.push(Sent::Ui(UiCommand::FindVocabInBook {
                book: *id,
                quote,
                row: row.key,
            }));
        }
    }
}

/// A book link's full name.
pub fn open_book(title: &str) -> String {
    format!("Open {title}")
}

/// The sentence link's full name.
pub fn open_sentence(lemma: &str) -> String {
    format!("Open the book where {lemma} was met")
}

/// The book's sentence, which opens the book there, and how many books
/// met the word.
fn sentence(ui: &mut Ui, row: &Row, t: &Tokens, out: &mut Commands) {
    let said = sentence::highlight(&row.context, &row.form, t);
    match &row.met_at {
        Some(at) => {
            let label = open_sentence(&row.lemma);
            let at_book = row.book_ids.iter().position(|b| *b == at.book);
            let title = at_book.and_then(|i| row.books.get(i));
            let tip = title.map_or_else(|| BOOK_HERE.to_owned(), |t| format!("Open {t} here"));
            let link = ui.add(Label::new(said).wrap().sense(Sense::click()));
            link.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Link, true, &label));
            if link.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            let link = link.on_hover_text(tip);
            if link.clicked() {
                let (book, quote) = (at.book, at.quote.clone());
                out.push(Sent::Ui(UiCommand::FindVocabInBook {
                    book,
                    quote,
                    row: row.key,
                }));
            }
        }
        None => {
            ui.add(Label::new(said).wrap());
        }
    }
    let n = row.books.len();
    if n > 1 {
        look::quiet(ui, &format!("Met in {n} books"), t.text_small, t);
    }
}

/// Memorize or un-memorize, then forget: each acts on the word everywhere.
fn actions(
    ui: &mut Ui,
    row: &Row,
    out: &mut Commands,
    t: &Tokens,
    selection: &mut Selection,
    scope: space::Scope,
) {
    ui.spacing_mut().item_spacing.x = 6.0;
    memorize(ui, row, out, t);
    forget(ui, row, selection, scope);
}

fn memorize(ui: &mut Ui, row: &Row, out: &mut Commands, t: &Tokens) {
    let words = vec![row.word];
    let (icon, verb, action, ink) = match row.status {
        WordStatus::Memorized => {
            let undo = Action::Unmemorize(words);
            (Icon::Restore, "Un-memorize", undo, t.ink)
        }
        _ => {
            let ink = look::swatch(t.toast.info).0;
            (Icon::Check, "Memorize", Action::Memorize(words), ink)
        }
    };
    let label = format!("{verb} {}", row.lemma);
    if look::text_button(ui, (icon, verb), &label, ink).clicked() {
        out.push(Sent::Engine(Command::Space(action)));
    }
}

fn forget(ui: &mut Ui, row: &Row, selection: &mut Selection, scope: space::Scope) {
    let label = format!("Forget {}", row.lemma);
    if widgets::small_icon_button(ui, Icon::Remove, &label).clicked() {
        selection.ask_delete(row, scope);
    }
}

/// Opens or closes the row's sentence; true when clicked.
fn chevron(ui: &mut Ui, open: bool, lemma: &str) -> bool {
    let (icon, verb) = match open {
        true => (Icon::ChevronDown, "Hide"),
        false => (Icon::ChevronRight, "Show"),
    };
    let label = format!("{verb} where {lemma} was met");
    widgets::small_icon_button(ui, icon, &label).clicked()
}
