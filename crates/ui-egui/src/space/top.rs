//! Space's head: the title, search and export, the tabs, stats and toolbar.

use egui::{Align, Id, Layout, RichText, Sense, Ui, vec2};
use mareader_engine::space::{Filter, Group, Scope, Sort, Tab, View};

use super::{EXPORT, TITLE, look};
use crate::icons::Icon;
use crate::theme::{self, Tokens};
use crate::widgets;

const LINE: &str = "Across every book. Memorized words are never shaded or carded again.";
const SEARCH_W: f32 = 280.0;
/// A merged row has many books, so it is never grouped by one.
const MERGED_GROUPS: [Group; 4] = [Group::None, Group::Level, Group::Month, Group::Status];
const BAR: egui::Vec2 = vec2(40.0, 6.0);

/// The title and its line, then search and Export; true on Export.
pub(super) fn header(ui: &mut Ui, filter: &mut Filter, t: &Tokens) -> bool {
    ui.label(RichText::new(TITLE).font(theme::serif(26.0)).color(t.ink));
    look::quiet(ui, LINE, t);
    ui.horizontal(|ui| {
        widgets::input(&mut filter.text)
            .id(Id::new("space-search"))
            .icon(Icon::Search)
            .hint("Search words, meanings and sentences")
            .label("Search your words")
            .clearable(true)
            .compact()
            .width(SEARCH_W)
            .show(ui);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let button = widgets::primary_button(ui, Some(Icon::Download), EXPORT, t.accent, t);
            ui.add(button).clicked()
        })
        .inner
    })
    .inner
}

/// A pill per status, each with its count.
pub(super) fn tabs(ui: &mut Ui, view: &View, filter: &mut Filter, t: &Tokens) {
    ui.horizontal_wrapped(|ui| {
        for tab in Tab::ALL {
            let text = format!("{} {}", tab.label(), view.counts.of(tab));
            if look::toggle(ui, filter.tab == tab, &text, t) {
                filter.tab = tab;
            }
        }
    });
}

/// Memorized words, first tries, and a bar per level's memorized words.
pub(super) fn stats(ui: &mut Ui, view: &View, t: &Tokens) {
    let (counts, stats) = (view.counts, view.stats);
    let (memorized, all) = (counts.memorized, counts.all);
    let (f, n, p) = (stats.first_try, stats.answered, stats.first_try_percent());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        look::quiet(ui, &format!("Memorized {memorized} of {all} words met"), t);
        let tries = format!("First try {p}% · {f} of {n} reactive answers");
        look::quiet(ui, &tries, t);
        let most = stats.by_level.iter().map(|(_, n)| *n).max().unwrap_or(0);
        for (band, n) in stats.by_level {
            ui.horizontal(|ui| level_bar(ui, band, n, most, t));
        }
    });
}

/// A level's name, a bar as long as its share of `most`, and its count.
fn level_bar(ui: &mut Ui, band: u8, n: usize, most: usize, t: &Tokens) {
    ui.spacing_mut().item_spacing.x = 4.0;
    look::quiet(ui, mareader_cefr::label(band), t);
    let (rect, _) = ui.allocate_exact_size(BAR, Sense::hover());
    let share = n as f32 / most.max(1) as f32;
    let mut filled = rect;
    filled.set_width(rect.width() * share);
    let painter = ui.painter();
    painter.rect_filled(rect, 3.0, t.track);
    painter.rect_filled(filled, 3.0, t.accent);
    look::quiet(ui, &n.to_string(), t);
}

/// Group and sort, then list or cards at the right.
pub(super) fn toolbar(ui: &mut Ui, filter: &mut Filter, cards: &mut bool) {
    let groups: &[Group] = match filter.scope {
        Scope::Merged => &MERGED_GROUPS,
        Scope::Books => &Group::ALL,
    };
    ui.horizontal(|ui| {
        pick(ui, "Group", &mut filter.group, groups, Group::label);
        pick(ui, "Sort", &mut filter.sort, &Sort::ALL, Sort::label);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.allocate_ui(vec2(150.0, 24.0), |ui| {
                widgets::segments(ui, cards, &[false, true], 2, view_face);
            });
        });
    });
}

/// A dropdown of `all`, named `name`; a value not offered shows the first.
fn pick<T: Copy + PartialEq>(
    ui: &mut Ui,
    name: &'static str,
    value: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) {
    let shown = match all.contains(value) {
        true => *value,
        false => all.first().copied().unwrap_or(*value),
    };
    ui.label(name);
    widgets::combo(("space", name))
        .selected_text(label(shown))
        .show_ui(ui, |ui| {
            for &v in all {
                ui.selectable_value(value, v, label(v));
            }
        });
}

fn view_face(cards: bool) -> (Option<Icon>, &'static str) {
    match cards {
        true => (Some(Icon::Grid), "Cards"),
        false => (Some(Icon::List), "List"),
    }
}
