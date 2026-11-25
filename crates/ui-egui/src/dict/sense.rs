//! A dictionary sense as a card: its tags, the answer, gloss and AI parts.

use super::look::{chip, say, tag};
use super::{Voice, ai_edit, entry};
use crate::shaped;
use crate::theme::Tokens;
use crate::ui_command::Commands;
use egui::{
    Align, CursorIcon, Frame, Margin, Rect, Response, RichText, Sense, Shape, Ui, WidgetInfo,
    WidgetType, vec2,
};
use mareader_dict::Entry;

/// What a sense card shows beside its answer.
pub struct Said<'a> {
    pub voice: &'a Voice,
    /// The answer's language, as `fr`.
    pub said_in: &'a str,
    /// Its pack named, when several were asked.
    pub way: bool,
    /// The whole card is a button, so it holds no voice of its own.
    pub click: bool,
    /// Its place in the list, to tell like cards apart.
    pub at: usize,
}

/// An entry's classes, any bridge, with `way` its two languages, and the
/// AI mark; a card that is all one button (`click`) keeps the pencil's spot.
///
/// The one tag row: sense cards and the page's entry both draw it.
pub(super) fn tag_row(
    ui: &mut Ui,
    e: &Entry,
    (way, click): (bool, bool),
    t: &Tokens,
    out: &mut Commands,
) -> Slot {
    let tags = entry::tags(e);
    let way = entry::way(e).filter(|_| way);
    if tags.is_empty() && e.via.is_none() && way.is_none() && e.ai.is_none() {
        return None;
    }
    let row = ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        // As tall as the chips, not a control's height.
        ui.spacing_mut().interact_size.y = 18.0;
        for tag in tags.split(" · ").filter(|s| !s.is_empty()) {
            chip(ui, tag, t);
        }
        if let Some(via) = &e.via {
            tag(ui, &format!("via {via}"), t)
                .on_hover_text(format!("Found through the English “{via}”"));
        }
        if let Some(way) = &way {
            tag(ui, way, t);
        }
        match e.ai.as_deref() {
            // A card that is all one button gets its pencil after the click.
            Some(said) if click => return Some(ai_edit::slot(ui, said, t)),
            Some(said) => ai_edit::mark(ui, said, t, out),
            None => {}
        }
        None
    });
    row.inner
}

/// Where a clickable card's pencil goes, drawn after its click.
pub(super) type Slot = Option<Rect>;

/// One sense: its tags, the answer large with its voice, then the gloss.
pub fn sense(ui: &mut Ui, e: &Entry, s: &Said<'_>, t: &Tokens, out: &mut Commands) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let mut slot = None;
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
            slot = tag_row(ui, e, (s.way, s.click), t, out);
            ui.horizontal(|ui| {
                let speaks =
                    !s.click && s.voice.can_say(s.said_in) && !e.answer().trim().is_empty();
                let room = ui.available_width() - if speaks { 34.0 } else { 0.0 };
                let lead = egui::Layout::left_to_right(Align::Center).with_main_wrap(true);
                ui.allocate_ui_with_layout(vec2(room.max(40.0), 0.0), lead, |ui| {
                    let size = entry::answer_size(e.answer(), t) * 1.1;
                    shaped::label(ui, e.answer(), size, t.ink);
                });
                if speaks {
                    let right = egui::Layout::right_to_left(Align::Center);
                    ui.with_layout(right, |ui| {
                        say(ui, (e.answer(), s.said_in), s.voice, 26.0, out);
                    });
                }
            });
            if let Some(r) = e.romanization.as_deref().filter(|r| !r.is_empty()) {
                shaped::label(ui, r, t.text_small, t.muted);
            }
            if let Some(gloss) = e.sense.as_deref().filter(|s| !s.is_empty()) {
                if shaped::needs_shaping(gloss) {
                    shaped::label(ui, gloss, t.text_small, t.muted);
                } else {
                    let text = RichText::new(gloss).italics().size(t.text_small);
                    ui.label(text.color(t.muted));
                }
            }
            if let Some(said) = &e.ai {
                crate::ai_said::show(ui, ("", &said.synonyms, &said.usages), t);
            }
        });
    let rect = inner.response.rect;
    let sense = if s.click {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with(("dict-sense", s.at)), sense);
    let lift = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.12);
    if s.click {
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, e.answer()));
    }
    if let (Some(slot), Some(said)) = (slot, e.ai.as_deref()) {
        ai_edit::pin(ui, slot, (said, s.at), out);
    }
    let fill = t.ink.gamma_multiply(0.035 + 0.025 * lift);
    ui.painter()
        .set(bg, Shape::rect_filled(rect, t.radius_lg, fill));
    r
}
