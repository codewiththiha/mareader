//! Space's small pieces: captions over groups, tags to read, raised paper,
//! and the plain text buttons rows act with.

use egui::text::{LayoutJob, TextFormat};
use egui::{Color32, CursorIcon, FontId, Rect, Response, Sense, Stroke, Ui, vec2};
use mareader_appearance::ToastSwatch;
use mareader_library::WordStatus;

use crate::icons::Icon;
use crate::theme::{self, Tokens};

/// Raised paper's corners: the stat cards and the rows' sheet.
pub(super) const ROUND: f32 = 16.0;

/// A small spaced caption in capitals, over a group of choices.
pub(super) fn caption(ui: &mut Ui, text: &str, t: &Tokens) {
    let format = TextFormat {
        font_id: theme::medium(11.5),
        color: t.muted,
        extra_letter_spacing: 0.9,
        ..TextFormat::default()
    };
    let job = LayoutJob::single_section(text.to_uppercase(), format);
    ui.add(egui::Label::new(job).selectable(false));
}

/// Muted words at `size`.
pub(super) fn quiet(ui: &mut Ui, text: &str, size: f32, t: &Tokens) -> Response {
    let words = egui::RichText::new(text).size(size).color(t.muted);
    ui.add(egui::Label::new(words).selectable(false))
}

/// A tag: `text` in `ink` on a rounded `fill`, `round` at the corners.
pub(super) fn tag(ui: &mut Ui, text: &str, colors: (Color32, Color32), round: f32) -> Response {
    let (ink, fill) = colors;
    let font = theme::medium(12.0);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, ink);
    let size = galley.size() + vec2(14.0, 6.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, round, fill);
    let at = rect.center() - galley.size() / 2.0;
    ui.painter().galley(at, galley, ink);
    resp
}

/// A word's level, in a quiet square tag.
pub(super) fn level(ui: &mut Ui, band: u8, t: &Tokens) -> Response {
    let colors = (t.ink.gamma_multiply(0.8), t.ink.gamma_multiply(0.07));
    tag(ui, mareader_cefr::label(band), colors, 6.0)
}

/// The ink and fill a status reads in: green, amber or red.
pub(super) fn status(status: WordStatus, t: &Tokens) -> (Color32, Color32) {
    match status {
        WordStatus::Memorized => swatch(t.toast.info),
        WordStatus::Learning => swatch(t.toast.warning),
        WordStatus::Missed => swatch(t.toast.error),
    }
}

/// A swatch's mark on its soft fill.
pub(super) fn swatch(s: ToastSwatch) -> (Color32, Color32) {
    (theme::color(s.mark), theme::color(s.soft))
}

/// Raised paper at `rect`: the rows' sheet and the stat cards.
pub(super) fn paper(ui: &Ui, rect: Rect, t: &Tokens) {
    let painter = ui.painter();
    painter.rect_filled(rect, ROUND, t.card);
    let edge = Stroke::new(1.0, t.line);
    painter.rect_stroke(rect, ROUND, edge, egui::StrokeKind::Inside);
}

/// A plain button: a mark and words, `label` naming it in full.
pub(super) fn text_button(
    ui: &mut Ui,
    (icon, words): (Icon, &str),
    label: &str,
    ink: Color32,
) -> Response {
    let t = Tokens::get(ui.ctx());
    let font = FontId::proportional(t.text_small + 1.0);
    let galley = ui.painter().layout_no_wrap(words.to_owned(), font, ink);
    let size = vec2(16.0 + 6.0 + galley.size().x + 12.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, t.hover);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let mark = Rect::from_center_size(rect.left_center() + vec2(14.0, 0.0), vec2(15.0, 15.0));
    icon.paint(ui, mark, ink);
    let at = egui::pos2(mark.right() + 6.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    resp
}
