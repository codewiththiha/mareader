//! The Highlights tab: each pen's colour and the selection menu.

use egui::{Align2, Color32, FontId, Sense, Stroke, Ui, Vec2, vec2};
use mareader_appearance::Rgb;
use mareader_settings::{BarItem, HighlightSettings, Pen, PickColor, Reserved};

use super::{choice_row, divider, group, note, row, section};
use crate::icons::Icon;
use crate::study::marks::pen_area;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::settings::Tab;

const DOT: f32 = 22.0;

/// The looks kept off the pens that are in use now.
#[derive(Clone, Copy, Debug, Default)]
pub struct Marks {
    /// A model answers the AI Dictionary, which marks words.
    pub ai: bool,
    /// The word levels are in, so words can be shaded.
    pub shading: bool,
}

pub fn tab(ui: &mut Ui, h: &mut HighlightSettings, marks: Marks, out: &mut Commands) {
    section(ui, "Highlighters");
    group(ui, |ui| {
        for (i, pen) in Pen::ALL.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            pen_row(ui, h, pen);
        }
        divider(ui);
        row(
            ui,
            "Fill strength",
            "How strongly fills tint the text",
            |ui| {
                ui.spacing_mut().slider_width = 140.0;
                let (lo, hi) = HighlightSettings::OPACITY;
                let mut pct = h.opacity * 100.0;
                let slider = egui::Slider::new(&mut pct, lo * 100.0..=hi * 100.0)
                    .step_by(5.0)
                    .fixed_decimals(0)
                    .suffix("%");
                if ui.add(slider).changed() {
                    h.opacity = pct / 100.0;
                }
            },
        );
    });
    section(ui, "When a highlighter is clicked");
    group(ui, |ui| {
        let picks = [
            (
                PickColor::Default,
                "Use its colour",
                "Highlights at once; new colours above recolour them.",
            ),
            (
                PickColor::Ask,
                "Choose a colour each time",
                "Shows the colours; the one picked stays with that highlight.",
            ),
        ];
        for (i, (pick, title, detail)) in picks.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, h.pick == pick) {
                h.pick = pick;
            }
        }
    });
    section(ui, "Selection menu");
    group(ui, |ui| menu(ui, &mut h.bar));
    note(ui, "Hidden buttons stay off the selection bar.");
    other_marks(ui, h, marks, out);
}

/// The AI's and the shading's looks, each set in its own tab.
fn other_marks(ui: &mut Ui, h: &HighlightSettings, marks: Marks, out: &mut Commands) {
    if !marks.ai && !marks.shading {
        return;
    }
    section(ui, "Other marks");
    group(ui, |ui| {
        if marks.ai {
            let hint = "Words the AI Dictionary explained";
            row(ui, "AI Dictionary", hint, |ui| {
                change(ui, ("Change in AI", Tab::Ai), out);
                preview(ui, h.ai.pen, h.ai.color, h.opacity);
            });
        }
        if marks.ai && marks.shading {
            divider(ui);
        }
        if marks.shading {
            let hint = "Words above your level, while shading is on";
            row(ui, "Vocabulary shading", hint, |ui| {
                change(ui, ("Change in Vocabulary", Tab::Vocabulary), out);
                preview(ui, h.cefr.pen, h.cefr.color, h.opacity);
            });
        }
    });
    note(
        ui,
        "Highlights can't take these looks, so none passes for them.",
    );
}

/// A link to the tab where a mark's look is set.
fn change(ui: &mut Ui, (text, tab): (&str, Tab), out: &mut Commands) {
    if ui.link(text).clicked() {
        out.push(Sent::Engine(Command::OpenSettings(tab)));
    }
    ui.add_space(8.0);
}

/// A pen's preview and the colours it may take.
fn pen_row(ui: &mut Ui, h: &mut HighlightSettings, pen: Pen) {
    let hint = "Its colour unless one is picked for a highlight";
    let opacity = h.opacity;
    let reserved = h.reserved();
    let mut color = h.color(pen);
    row(ui, pen.label(), hint, |ui| {
        let free = |c: Rgb| !reserved.contains(&Reserved { pen, color: c });
        palette(
            ui,
            (pen.label(), "Kept for AI marks or shading"),
            &mut color,
            free,
        );
        preview(ui, pen, color, opacity);
    });
    *h.color_mut(pen) = color;
}

/// "Aa" drawn over with `pen`, as a page shows it.
pub(super) fn preview(ui: &mut Ui, pen: Pen, color: Rgb, opacity: f32) {
    let t = Tokens::get(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(34.0, 22.0), Sense::hover());
    let area = pen_area(rect.shrink2(vec2(3.0, 3.0)), pen);
    // As a page lays it: under the ink, on the group's fill.
    let fill = if pen == Pen::Underline {
        theme::color(color)
    } else {
        let [r, g, b, _] = t.card.to_array();
        let ground = [r, g, b].map(|c| f32::from(c) / 255.0);
        let lit = t.page.mark(color.to_unit(), opacity).over(ground);
        let [r, g, b] = lit.map(|c| (c * 255.0).round() as u8);
        Color32::from_rgb(r, g, b)
    };
    let painter = ui.painter();
    painter.rect_filled(area, 0.0, fill);
    let font = FontId::proportional(t.text);
    painter.text(rect.center(), Align2::CENTER_CENTER, "Aa", font, t.ink);
}

/// The palette as dots, laid right to left; kept ones can't be picked.
pub(super) fn palette(
    ui: &mut Ui,
    (name, kept): (&str, &str),
    value: &mut Rgb,
    free: impl Fn(Rgb) -> bool,
) {
    let t = Tokens::get(ui.ctx());
    ui.spacing_mut().item_spacing.x = 4.0;
    let mut srgb = [value.0, value.1, value.2];
    ui.spacing_mut().interact_size = Vec2::splat(20.0);
    let picker = egui::color_picker::color_edit_button_srgb(ui, &mut srgb);
    let custom = Rgb(srgb[0], srgb[1], srgb[2]);
    if custom != *value && free(custom) {
        *value = custom;
    }
    widgets::tip(&picker, "Custom", None);
    for c in HighlightSettings::PALETTE.into_iter().rev() {
        let (rect, r) = ui.allocate_exact_size(Vec2::splat(DOT), Sense::click());
        let (open, on) = (free(c), *value == c);
        let painter = ui.painter();
        let fill = theme::color(c);
        let fill = if open { fill } else { fill.gamma_multiply(0.3) };
        painter.circle_filled(rect.center(), 9.0, fill);
        if on {
            painter.circle_stroke(rect.center(), 10.5, Stroke::new(1.5, t.ink));
        }
        if !open {
            let d = vec2(5.0, -5.0);
            let slash = [rect.center() - d, rect.center() + d];
            painter.line_segment(slash, Stroke::new(1.5, t.muted));
        }
        let label = format!("{name} {}", c.to_hex());
        r.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, open, on, &label));
        let tip = if open { c.to_hex() } else { kept.to_owned() };
        widgets::tip(&r, &tip, None);
        if open && r.clicked() {
            *value = c;
        }
    }
}

/// What a menu row asks for.
enum Move {
    Show(BarItem, bool),
    Up(usize),
    Down(usize),
}

/// The bar's buttons in order, then the hidden; each shown or not.
fn menu(ui: &mut Ui, bar: &mut Vec<BarItem>) {
    let hidden = BarItem::ALL.into_iter().filter(|i| !bar.contains(i));
    let listed: Vec<BarItem> = bar.iter().copied().chain(hidden).collect();
    let mut asked = None;
    for (i, item) in listed.iter().copied().enumerate() {
        if i > 0 {
            divider(ui);
        }
        let shown = i < bar.len();
        let label = item.label();
        let hint = if shown { "On the menu" } else { "Hidden" };
        row(ui, label, hint, |ui| {
            let mut on = shown;
            if widgets::switch(ui, &mut on, label).changed() || on != shown {
                asked = Some(Move::Show(item, on));
            }
            if !shown {
                return;
            }
            let down =
                widgets::small_icon_button(ui, Icon::MoveDown, &format!("Move {label} down"));
            if down.clicked() && i + 1 < bar.len() {
                asked = Some(Move::Down(i));
            }
            let up = widgets::small_icon_button(ui, Icon::MoveUp, &format!("Move {label} up"));
            if up.clicked() && i > 0 {
                asked = Some(Move::Up(i));
            }
        });
    }
    match asked {
        Some(Move::Show(item, true)) => bar.push(item),
        Some(Move::Show(item, false)) => bar.retain(|i| *i != item),
        Some(Move::Up(i)) => bar.swap(i, i - 1),
        Some(Move::Down(i)) => bar.swap(i, i + 1),
        None => {}
    }
}
