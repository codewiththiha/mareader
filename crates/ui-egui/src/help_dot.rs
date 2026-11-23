//! A small "?" beside a heading, drawn with egui: hovering shows what the
//! heading's controls do, and a click pins that card open until a click
//! elsewhere or Escape.

use egui::{
    Align2, CursorIcon, FontId, Frame, Margin, Popup, PopupCloseBehavior, RectAlign, Response,
    RichText, Sense, Shadow, Stroke, Tooltip, Ui, vec2,
};

use crate::theme::{self, Tokens};

/// The dot's diameter.
const SIZE: f32 = 16.0;
/// The card's text width.
const CARD_W: f32 = 300.0;

/// The dot for `title`; `lines` are the card's paragraphs, each a lead
/// in bold (may be empty) and its words.
pub fn show(ui: &mut Ui, title: &str, lines: &[(&str, &str)]) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(SIZE, SIZE), Sense::click());
    let t = Tokens::get(ui.ctx());
    let id = resp.id.with("card");
    let open = Popup::is_id_open(ui.ctx(), id);
    let hot = resp.hovered() || open;
    if resp.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = if hot { t.accent } else { t.muted };
    let painter = ui.painter();
    if hot {
        painter.circle_filled(rect.center(), SIZE / 2.0, t.hover);
    }
    painter.circle_stroke(rect.center(), SIZE / 2.0 - 0.5, Stroke::new(1.0, ink));
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "?",
        theme::medium(11.0),
        ink,
    );
    let name = format!("About {title}");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    let frame = card(&t);
    // A pinned card already shows; a tooltip over it would repeat it.
    if !open {
        let mut tip = Tooltip::for_enabled(&resp);
        tip.popup = tip.popup.frame(frame).gap(6.0);
        tip.show(|ui| body(ui, &t, title, lines));
    }
    Popup::from_toggle_button_response(&resp)
        .id(id)
        .frame(frame)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClickOutside)
        .align(RectAlign::BOTTOM_START)
        .gap(6.0)
        .show(|ui| body(ui, &t, title, lines));
    resp
}

/// The card's look: the app's tooltip chip, roomier.
fn card(t: &Tokens) -> Frame {
    Frame::new()
        .fill(t.surface)
        .stroke(Stroke::new(1.0, t.line))
        .corner_radius(8)
        .inner_margin(Margin::symmetric(12, 10))
        .shadow(Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: t.shadow,
        })
}

/// The heading, then each paragraph wrapped to the card's width.
fn body(ui: &mut Ui, t: &Tokens, title: &str, lines: &[(&str, &str)]) {
    ui.set_width(CARD_W);
    ui.spacing_mut().item_spacing.y = 6.0;
    ui.label(
        RichText::new(title)
            .font(theme::medium(t.text))
            .color(t.ink),
    );
    for (lead, words) in lines {
        let mut job = egui::text::LayoutJob::default();
        let size = t.text_small;
        if !lead.is_empty() {
            let bold = egui::TextFormat::simple(theme::medium(size), t.ink);
            job.append(&format!("{lead}  "), 0.0, bold);
        }
        let plain = egui::TextFormat::simple(FontId::proportional(size), t.muted);
        job.append(words, 0.0, plain);
        job.wrap.max_width = CARD_W;
        ui.label(job);
    }
}
