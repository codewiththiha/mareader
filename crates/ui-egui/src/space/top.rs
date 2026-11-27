//! Space's head: the title, search and export, the tabs, the stat cards and
//! the toolbar.

use egui::{Align, Align2, Button, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui};
use egui::{pos2, vec2};
use mareader_engine::space::{Filter, Group, Scope, Sort, Tab, View};

use super::{EXPORT, TITLE, look};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

const LINE: &str = "Across every book. Memorized words are never shaded or carded again.";
const SEARCH_W: f32 = 320.0;
/// A merged row has many books, so it is never grouped by one.
const MERGED_GROUPS: [Group; 5] = [
    Group::None,
    Group::Level,
    Group::Day,
    Group::Month,
    Group::Status,
];
/// The toolbar's controls' height.
const TOOL_H: f32 = 34.0;
/// A stat card's height, and the room between cards.
const STAT_H: f32 = 134.0;
const STAT_GAP: f32 = 16.0;
/// Stat cards shrink below the first width and stack below the second.
const STATS_ROOMY: f32 = 620.0;
const STATS_ROW_FROM: f32 = 440.0;
/// The toolbar keeps to one line from this width on, else takes two.
const TOOLS_ONE_LINE: f32 = 760.0;

/// The title and its line beside search, then Export; true on Export.
pub(super) fn header(ui: &mut Ui, filter: &mut Filter, t: &Tokens) -> bool {
    ui.horizontal_top(|ui| {
        let wide = ui.available_width();
        let words = (wide - SEARCH_W - 24.0).max(wide * 0.5);
        ui.allocate_ui_with_layout(vec2(words, 0.0), Layout::top_down(Align::Min), |ui| {
            ui.spacing_mut().item_spacing.y = 4.0;
            let title = RichText::new(TITLE).font(theme::semibold(30.0));
            ui.add(egui::Label::new(title.color(t.ink)).selectable(false));
            let line = RichText::new(LINE).size(t.text + 1.0).color(t.muted);
            ui.add(egui::Label::new(line).wrap().selectable(false));
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            let width = SEARCH_W.min(ui.available_width());
            widgets::input(&mut filter.text)
                .id(Id::new("space-search"))
                .icon(Icon::Search)
                .hint("Search words, meanings, sentences")
                .label("Search your words")
                .clearable(true)
                .width(width)
                .show(ui);
        });
    });
    ui.add_space(4.0);
    export(ui, t)
}

/// Export CSV as a quiet outlined button.
fn export(ui: &mut Ui, t: &Tokens) -> bool {
    let mark = Icon::Download.image(ui, 16.0).tint(t.ink);
    let words = RichText::new(EXPORT).size(t.text + 1.0).color(t.ink);
    let button = Button::image_and_text(mark, words)
        .fill(t.card)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(10.0)
        .min_size(vec2(0.0, 38.0));
    ui.add(button).clicked()
}

/// The statuses as tabs over a hairline, each with its count.
pub(super) fn tabs(ui: &mut Ui, view: &View, filter: &mut Filter, t: &Tokens) {
    let items = Tab::ALL.map(|tab| (tab, tab.label(), view.counts.of(tab)));
    tab_row(ui, &items, &mut filter.tab, t);
}

/// Tabs over a hairline, each a name and its count; a click picks one.
pub(super) fn tab_row<T: Copy + PartialEq>(
    ui: &mut Ui,
    items: &[(T, &str, usize)],
    picked: &mut T,
    t: &Tokens,
) {
    let mut under = None;
    let row = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 30.0;
        for &(item, name, n) in items {
            let on = *picked == item;
            let (rect, clicked) = tab_button(ui, name, n, on, t);
            if clicked {
                *picked = item;
            }
            if on {
                under = Some(rect);
            }
        }
    });
    let full = ui.max_rect().x_range();
    let y = row.response.rect.bottom();
    ui.painter().hline(full, y - 0.5, Stroke::new(1.0, t.line));
    if let Some(rect) = under {
        let under = Rect::from_x_y_ranges(rect.x_range(), y - 2.0..=y);
        ui.painter().rect_filled(under, 1.0, t.accent);
    }
}

/// One tab: its name, then its count, smaller; its rect, and if clicked.
fn tab_button(ui: &mut Ui, name: &str, n: usize, on: bool, t: &Tokens) -> (Rect, bool) {
    let name_font = match on {
        true => theme::medium(t.text + 2.0),
        false => FontId::proportional(t.text + 2.0),
    };
    let painter = ui.painter();
    let label = format!("{name} {n}");
    let name = painter.layout_no_wrap(name.to_owned(), name_font, t.ink);
    let count = painter.layout_no_wrap(n.to_string(), FontId::proportional(t.text_small), t.muted);
    let size = vec2(name.size().x + 6.0 + count.size().x, 40.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let ink = match (on, r.hovered()) {
        (true, _) => t.accent,
        (false, true) => t.ink,
        (false, false) => t.ink.gamma_multiply(0.75),
    };
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let y = rect.center().y;
    let name_at = pos2(rect.left(), y - name.size().y / 2.0);
    let count_x = rect.left() + name.size().x + 6.0;
    let count_at = pos2(count_x, y - count.size().y / 2.0 + 1.0);
    let painter = ui.painter();
    painter.galley(name_at, name, ink);
    painter.galley(count_at, count, t.muted);
    r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, on, &label));
    (rect, r.clicked())
}

/// Memorized words, first tries, and a bar per level's memorized words, as
/// three cards in a row, or one over another on a narrow page.
pub(super) fn stats(ui: &mut Ui, view: &View, t: &Tokens) {
    let (counts, stats) = (view.counts, view.stats);
    let wide = ui.available_width();
    let across = if wide >= STATS_ROW_FROM { 3 } else { 1 };
    let rows = 3 / across;
    let card_h = match wide >= STATS_ROOMY {
        true => STAT_H,
        false => STAT_H - 20.0,
    };
    let card_w = (wide - STAT_GAP * (across - 1) as f32) / across as f32;
    let tall = card_h * rows as f32 + STAT_GAP * (rows - 1) as f32;
    let (block, _) = ui.allocate_exact_size(vec2(wide, tall), Sense::hover());
    let cell = |i: usize| {
        let (col, row) = ((i % across) as f32, (i / across) as f32);
        let step = vec2(card_w + STAT_GAP, card_h + STAT_GAP);
        let min = block.min + vec2(col * step.x, row * step.y);
        Rect::from_min_size(min, vec2(card_w, card_h))
    };
    let memorized = format!("of {} words met", counts.all);
    let figure = Figure {
        caption: "Memorized",
        value: counts.memorized.to_string(),
        unit: "",
        under: &memorized,
    };
    number(ui, cell(0), &figure, t);
    let answers = format!("{} of {} reactive answers", stats.first_try, stats.answered);
    let figure = Figure {
        caption: "First-try recall",
        value: stats.first_try_percent().to_string(),
        unit: "%",
        under: &answers,
    };
    number(ui, cell(1), &figure, t);
    levels(ui, cell(2), &stats.by_level, t);
}

/// A stat card's words: what it counts, the figure, and what it is of.
struct Figure<'a> {
    caption: &'a str,
    value: String,
    unit: &'a str,
    under: &'a str,
}

/// A card with a big figure between its caption and its line.
fn number(ui: &Ui, rect: Rect, figure: &Figure<'_>, t: &Tokens) {
    look::paper(ui, rect, t);
    let painter = ui.painter();
    let left = rect.left() + 22.0;
    let small = FontId::proportional(t.text_small + 1.0);
    let top = pos2(left, rect.top() + 20.0);
    let caption = figure.caption;
    painter.text(top, Align2::LEFT_TOP, caption, small.clone(), t.muted);
    let base = pos2(left, rect.top() + 44.0);
    let value = figure.value.as_str();
    let big = theme::semibold((rect.height() * 0.28).clamp(28.0, 38.0));
    let big = painter.text(base, Align2::LEFT_TOP, value, big, t.ink);
    if !figure.unit.is_empty() {
        let at = pos2(big.right() + 2.0, big.bottom() - 8.0);
        let unit = theme::semibold(20.0);
        painter.text(at, Align2::LEFT_BOTTOM, figure.unit, unit, t.ink);
    }
    let foot = pos2(left, rect.bottom() - 20.0);
    painter.text(foot, Align2::LEFT_BOTTOM, figure.under, small, t.muted);
}

/// A card with a bar per level, as tall as its share of the most.
fn levels(ui: &Ui, rect: Rect, by_level: &[(u8, usize); 4], t: &Tokens) {
    look::paper(ui, rect, t);
    let painter = ui.painter();
    let small = FontId::proportional(t.text_small + 1.0);
    let top = pos2(rect.left() + 22.0, rect.top() + 20.0);
    painter.text(top, Align2::LEFT_TOP, "Memorized by level", small, t.muted);
    let area = Rect::from_min_max(
        pos2(rect.left() + 22.0, rect.top() + 50.0),
        pos2(rect.right() - 22.0, rect.bottom() - 34.0),
    );
    let most = by_level.iter().map(|(_, n)| *n).max().unwrap_or(0).max(1);
    let slot = area.width() / by_level.len() as f32;
    let label = FontId::proportional(t.text_small - 1.0);
    for (i, (band, n)) in by_level.iter().enumerate() {
        let x = area.left() + slot * (i as f32 + 0.5);
        let height = (area.height() * *n as f32 / most as f32).max(4.0);
        let bar = Rect::from_min_max(
            pos2(x - slot * 0.4, area.bottom() - height),
            pos2(x + slot * 0.4, area.bottom()),
        );
        let fill = t.accent.gamma_multiply(if *n == 0 { 0.25 } else { 0.7 });
        painter.rect_filled(bar, 4.0, fill);
        let name = mareader_cefr::label(*band);
        let at = pos2(x, area.bottom() + 8.0);
        painter.text(at, Align2::CENTER_TOP, name, label.clone(), t.muted);
        let resp = ui.interact(bar, ui.id().with(("space-level", band)), Sense::hover());
        let said = format!("{name}: {n} memorized");
        widgets::tip(&resp, &said, None);
    }
}

/// Rows with books or merged, group and sort, then list or cards right.
pub(super) fn toolbar(ui: &mut Ui, filter: &mut Filter, cards: &mut bool, t: &Tokens) {
    let groups: &[Group] = match filter.scope {
        Scope::Merged => &MERGED_GROUPS,
        Scope::Books => &Group::ALL,
    };
    let one_line = ui.available_width() >= TOOLS_ONE_LINE;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        ui.allocate_ui(vec2(220.0, TOOL_H), |ui| {
            let scope = &mut filter.scope;
            widgets::segments_of(ui, TOOL_H, scope, &Scope::ALL, 2, |s| (None, s.label()));
        });
        if one_line {
            ui.add_space(8.0);
            sorting(ui, filter, groups, t);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.allocate_ui(vec2(170.0, TOOL_H), |ui| {
                widgets::segments_of(ui, TOOL_H, cards, &[false, true], 2, view_face);
            });
        });
    });
    if !one_line {
        ui.horizontal(|ui| sorting(ui, filter, groups, t));
    }
}

/// The group and sort dropdowns, side by side.
fn sorting(ui: &mut Ui, filter: &mut Filter, groups: &[Group], t: &Tokens) {
    ui.spacing_mut().item_spacing.x = 10.0;
    pick(ui, ("Group", t), &mut filter.group, groups, Group::label);
    ui.add_space(4.0);
    pick(ui, ("Sort", t), &mut filter.sort, &Sort::ALL, Sort::label);
}

/// A dropdown of `all`, named `name`; a value not offered shows the first.
fn pick<T: Copy + PartialEq>(
    ui: &mut Ui,
    (name, t): (&'static str, &Tokens),
    value: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) {
    let shown = match all.contains(value) {
        true => *value,
        false => all.first().copied().unwrap_or(*value),
    };
    look::quiet(ui, name, t.text + 1.0, t);
    let items: Vec<_> = all.iter().map(|&v| (v, label(v).to_owned())).collect();
    crate::dropdown::select(
        ui,
        egui::Id::new(("space", name)),
        label(shown),
        120.0,
        value,
        &items,
    );
}

fn view_face(cards: bool) -> (Option<Icon>, &'static str) {
    match cards {
        true => (Some(Icon::Grid), "Cards"),
        false => (Some(Icon::List), "List"),
    }
}
