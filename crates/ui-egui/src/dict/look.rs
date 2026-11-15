//! The Dictionary's own parts: its search field, pickers, keys and cards.

use super::{Offer, Voice, entry};
use crate::icons::Icon;
use crate::read_aloud::{SAY, STOP_SAYING};
use crate::shaped;
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent};
use egui::text::{LayoutJob, TextWrapping};
use egui::{
    Align, Color32, CornerRadius, CursorIcon, FontId, Frame, Id, Margin, Popup, PopupCloseBehavior,
    Rect, RectAlign, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui, Vec2,
    WidgetInfo, WidgetType, pos2, vec2,
};
use mareader_dict::Entry;
use mareader_engine::command::Command;

/// A search field: a pill whose ring lights while it has the keys.
pub struct Field<'a> {
    pub text: &'a mut String,
    pub id: Id,
    pub hint: &'a str,
    pub height: f32,
    /// The typed text's size.
    pub size: f32,
}

/// The field drawn, and whether its clear button was pressed.
pub struct Typed {
    pub field: Response,
    pub cleared: bool,
}

/// The pill's radius for a height, capped as egui's corners are.
fn round(height: f32) -> CornerRadius {
    CornerRadius::same((height / 2.0).clamp(0.0, 255.0) as u8)
}

pub fn field(ui: &mut Ui, f: Field<'_>, t: &Tokens) -> Typed {
    let size = vec2(ui.available_width(), f.height);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let ctx = ui.ctx().clone();
    let focused = ui.memory(|m| m.has_focus(f.id));
    let secs = crate::motion::secs(&ctx, 0.16);
    let lit = ctx.animate_bool_with_time(f.id.with("lit"), focused, secs);
    // A soft ring of the accent grows round it while it has the keys.
    if lit > 0.0 {
        let ring = rect.expand(3.0);
        let glow = t.accent.gamma_multiply(0.16 * lit);
        ui.painter().rect_filled(ring, round(ring.height()), glow);
    }
    let fill = t.surface.lerp_to_gamma(t.paper, lit);
    let edge = Stroke::new(1.0 + 0.5 * lit, t.line.lerp_to_gamma(t.accent, lit));
    (ui.painter()).rect(rect, round(f.height), fill, edge, StrokeKind::Inside);
    let pad = (f.height * 0.36).round();
    let glyph = f.size + 1.0;
    let mark = Rect::from_center_size(
        pos2(rect.left() + pad + glyph / 2.0, rect.center().y),
        Vec2::splat(glyph),
    );
    let ink = t.muted.lerp_to_gamma(t.accent, lit);
    Icon::Search.image(ui, glyph).tint(ink).paint_at(ui, mark);
    let typed = !f.text.is_empty();
    let button = f.height - 14.0;
    let end = match typed {
        true => rect.right() - 7.0 - button - 4.0,
        false => rect.right() - pad,
    };
    let left = mark.right() + 9.0;
    // As tall as the pill, so tall scripts and the hint sit centred, uncut.
    let room = Rect::from_min_max(
        pos2(left, rect.top()),
        pos2(end.max(left + 24.0), rect.bottom()),
    );
    let edit = TextEdit::singleline(f.text)
        .id(f.id)
        .hint_text(f.hint)
        .font(FontId::proportional(f.size))
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .return_key(None)
        .vertical_align(Align::Center)
        .min_size(room.size())
        .desired_width(room.width());
    let field = ui.put(room, edit);
    let shown = ctx.animate_bool_with_time(f.id.with("clear"), typed, secs);
    let mut cleared = false;
    if shown > 0.0 {
        let centre = pos2(rect.right() - 7.0 - button / 2.0, rect.center().y);
        let spot = Rect::from_center_size(centre, Vec2::splat(button));
        let r = ui.interact(spot, f.id.with("clear-button"), Sense::click());
        let fill = match r.hovered() {
            true => t.ink.gamma_multiply(0.12),
            false => t.ink.gamma_multiply(0.06),
        };
        let radius = button / 2.0 * (0.7 + 0.3 * shown);
        ui.painter()
            .circle_filled(centre, radius, fill.gamma_multiply(shown));
        let x = Rect::from_center_size(centre, Vec2::splat(12.0));
        let tint = t.muted.gamma_multiply(shown);
        Icon::Close.image(ui, 12.0).tint(tint).paint_at(ui, x);
        if r.hovered() {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
        }
        r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, typed, "Clear search"));
        cleared = typed && r.clicked();
    }
    Typed { field, cleared }
}

/// One choice in a picker's list; true when picked.
pub fn option(ui: &mut Ui, on: bool, text: &str, t: &Tokens) -> bool {
    let size = vec2(ui.available_width(), 30.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    if r.hovered() {
        ui.painter().rect_filled(rect, t.radius - 2.0, t.hover);
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = if on { t.accent } else { t.ink };
    let font = if on {
        theme::medium(t.text)
    } else {
        FontId::proportional(t.text)
    };
    let room = rect.width() - 40.0;
    let galley = ui.painter().layout_job(cut(text, font, ink, room));
    let at = pos2(rect.left() + 10.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, ink);
    if on {
        let tick = Rect::from_center_size(
            pos2(rect.right() - 18.0, rect.center().y),
            Vec2::splat(14.0),
        );
        Icon::Check
            .image(ui, 14.0)
            .tint(t.accent)
            .paint_at(ui, tick);
    }
    r.widget_info(|| WidgetInfo::selected(WidgetType::SelectableLabel, true, on, text));
    r.clicked()
}

/// `text` on one line, ending in "…" past `width`.
fn cut(text: &str, font: FontId, ink: Color32, width: f32) -> LayoutJob {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, ink);
    job.wrap = TextWrapping::truncate_at_width(width.max(8.0));
    job
}

/// A dropdown drawn like the field: its value, cut short, then a chevron.
pub fn picker(
    ui: &mut Ui,
    (id, name): (Id, &str),
    size: Vec2,
    shown: &str,
    t: &Tokens,
    list: impl FnOnce(&mut Ui),
) -> Response {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let open = Popup::is_id_open(ui.ctx(), id);
    let lit = ui
        .ctx()
        .animate_bool_with_time(id.with("lit"), r.hovered() || open, 0.1);
    let fill = t.surface.lerp_to_gamma(t.hover, lit);
    let edge = Stroke::new(
        1.0,
        t.line.lerp_to_gamma(t.accent, if open { 0.7 } else { 0.0 }),
    );
    (ui.painter()).rect(rect, t.radius, fill, edge, StrokeKind::Inside);
    let font = FontId::proportional(t.text);
    let room = rect.width() - 12.0 - 26.0;
    let galley = ui.painter().layout_job(cut(shown, font, t.ink, room));
    let at = pos2(rect.left() + 12.0, rect.center().y - galley.size().y / 2.0);
    ui.painter().galley(at, galley, t.ink);
    let chevron = if open {
        Icon::ChevronUp
    } else {
        Icon::ChevronDown
    };
    let spot = Rect::from_center_size(
        pos2(rect.right() - 16.0, rect.center().y),
        Vec2::splat(14.0),
    );
    chevron.image(ui, 14.0).tint(t.muted).paint_at(ui, spot);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    r.widget_info(|| WidgetInfo::labeled(WidgetType::ComboBox, true, name));
    let width = rect.width().max(160.0);
    Popup::from_toggle_button_response(&r)
        .id(id)
        .sense(crate::widgets::POPUP_SENSE)
        .close_behavior(PopupCloseBehavior::CloseOnClick)
        .align(RectAlign::BOTTOM_START)
        .gap(4.0)
        .width(width)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.set_min_width(width - 12.0);
            list(ui);
        });
    r
}

/// Keys and what they do, centred on one line: `↑ ↓ choose · Esc clear`.
pub fn keys(ui: &mut Ui, all: &[(&[&str], &str)], t: &Tokens) {
    let font = FontId::proportional(t.text_small);
    let cap_font = theme::medium(t.text_small - 1.0);
    let lay = |s: &str, f: &FontId, c| ui.painter().layout_no_wrap(s.to_owned(), f.clone(), c);
    // Each piece: is it a key, its text, its width, and the gap before it.
    let mut pieces = Vec::new();
    let mut width = 0.0;
    for (i, (caps, what)) in all.iter().enumerate() {
        for (k, cap) in caps.iter().enumerate() {
            let g = lay(cap, &cap_font, t.ink);
            let w = (g.size().x + 10.0).max(20.0);
            let gap = match (i, k) {
                (0, 0) => 0.0,
                (_, 0) => 14.0,
                _ => 3.0,
            };
            width += gap + w;
            pieces.push((true, g, w, gap));
        }
        let g = lay(what, &font, t.muted);
        let w = g.size().x;
        width += 5.0 + w;
        pieces.push((false, g, w, 5.0));
    }
    let row = vec2(ui.available_width(), 20.0);
    let (rect, _) = ui.allocate_exact_size(row, Sense::hover());
    let mut x = rect.center().x - width.min(rect.width()) / 2.0;
    let y = rect.center().y;
    let painter = ui.painter_at(rect);
    for (cap, g, w, gap) in pieces {
        x += gap;
        if cap {
            let r = Rect::from_min_size(pos2(x, y - 9.0), vec2(w, 18.0));
            painter.rect(
                r,
                5.0,
                t.surface,
                Stroke::new(1.0, t.line),
                StrokeKind::Inside,
            );
            // A key's lower lip, as on a keyboard.
            let lip = Rect::from_min_max(
                pos2(r.left() + 1.0, r.bottom() - 2.0),
                r.max - vec2(1.0, 1.0),
            );
            painter.rect_filled(lip, 2.0, t.line.gamma_multiply(0.6));
            let at = pos2(r.center().x - g.size().x / 2.0, y - g.size().y / 2.0 - 0.5);
            painter.galley(at, g, t.ink);
        } else {
            painter.galley(pos2(x, y - g.size().y / 2.0), g, t.muted);
        }
        x += w;
    }
    let text: Vec<String> = all
        .iter()
        .map(|(caps, what)| format!("{} {what}", caps.join(" ")))
        .collect();
    let label = text.join(" · ");
    let r = ui.interact(rect, ui.id().with("keys"), Sense::hover());
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &label));
}

/// A round button that says `word`, in `language`, aloud: soft, or lit
/// while it speaks.
pub fn say(
    ui: &mut Ui,
    (word, language): (&str, &str),
    voice: &Voice,
    size: f32,
    out: &mut Commands,
) {
    let t = Tokens::get(ui.ctx());
    let saying = voice.saying.as_deref() == Some(word);
    let (rect, r) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let how = ui.ctx().animate_bool_with_time(r.id, saying, 0.15);
    let hover = r.hovered() as u8 as f32;
    let soft = t.accent_soft.lerp_to_gamma(t.accent, 0.12 * hover);
    let fill = soft.lerp_to_gamma(t.accent, how);
    let press = if r.is_pointer_button_down_on() {
        0.92
    } else {
        1.0
    };
    ui.painter()
        .circle_filled(rect.center(), size / 2.0 * press, fill);
    let ink = t.accent.lerp_to_gamma(theme::ink_on(t.accent), how);
    let glyph = (size * 0.5).round();
    let spot = Rect::from_center_size(rect.center(), Vec2::splat(glyph));
    Icon::Speak.image(ui, glyph).tint(ink).paint_at(ui, spot);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let label = if saying { STOP_SAYING } else { SAY };
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let r = r.on_hover_text(if saying { "Stop" } else { "Say it aloud" });
    if !r.clicked() {
        return;
    }
    let speaker = voice.speaker(language).and_then(|s| s.voice.as_deref());
    out.push(match saying {
        true => Sent::Engine(Command::StopSaying),
        false => crate::read_aloud::say(word, speaker),
    });
}

/// "Get a French voice to hear these": a soft row opening the voice in
/// Settings.
pub fn offer(ui: &mut Ui, offer: Offer, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let name = crate::settings_window::accent(offer.tag);
    let a = if name.starts_with(['A', 'E', 'I', 'O', 'U']) {
        "an"
    } else {
        "a"
    };
    let text = format!("Get {a} {name} voice to hear these");
    let size = vec2(ui.available_width(), 34.0);
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let hover = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.12);
    let fill = t.accent_soft.lerp_to_gamma(t.accent, 0.10 * hover);
    let painter = ui.painter();
    painter.rect_filled(rect, t.radius_lg, fill.gamma_multiply(0.7));
    let glyph = Rect::from_center_size(rect.left_center() + vec2(18.0, 0.0), Vec2::splat(15.0));
    Icon::Download
        .image(ui, 15.0)
        .tint(t.accent)
        .paint_at(ui, glyph);
    let font = egui::FontId::proportional(t.text_small);
    let room = rect.width() - 64.0;
    let mut job = LayoutJob::simple_singleline(text.clone(), font, t.accent);
    job.wrap = TextWrapping::truncate_at_width(room.max(10.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let at = egui::pos2(rect.left() + 34.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(at, galley, t.accent);
    let nudge = 2.0 * hover;
    let chevron = rect.right_center() + vec2(-16.0 + nudge, 0.0);
    let chevron = Rect::from_center_size(chevron, Vec2::splat(14.0));
    Icon::ChevronRight
        .image(ui, 14.0)
        .tint(t.accent)
        .paint_at(ui, chevron);
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &text));
    let tip = format!(
        "{} has {name} voices. Opens Settings at them.",
        offer.model.name
    );
    if r.on_hover_text(tip).clicked() {
        out.push(Sent::Engine(Command::RevealVoice(offer.model, offer.tag)));
    }
}

/// A small rounded tag in the accent's soft tint.
pub fn chip(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let text = RichText::new(text).size(t.text_small).color(t.accent);
    Frame::new()
        .fill(t.accent_soft)
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| ui.add(egui::Label::new(text).selectable(false)))
        .inner
}

/// A quiet tag: the language pair, or a bridge's English word.
pub fn tag(ui: &mut Ui, text: &str, t: &Tokens) -> Response {
    let text = RichText::new(text).size(t.text_small).color(t.muted);
    Frame::new()
        .fill(t.ink.gamma_multiply(0.05))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(8, 1))
        .show(ui, |ui| ui.add(egui::Label::new(text).selectable(false)))
        .inner
}

/// "English → Burmese", from an entry's pack.
pub fn way(e: &Entry) -> Option<String> {
    let p = mareader_dict::PACKS.iter().find(|p| p.id == e.pack)?;
    let (from, to) = match e.flipped {
        true => (p.target, p.source),
        false => (p.source, p.target),
    };
    let name = mareader_dict::language;
    Some(format!("{} → {}", name(from), name(to)))
}

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

/// One sense: its tags, the answer large with its voice, then the gloss.
pub fn sense(ui: &mut Ui, e: &Entry, s: &Said<'_>, t: &Tokens, out: &mut Commands) -> Response {
    let bg = ui.painter().add(Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing = vec2(6.0, 2.0);
            let tags = e.tags();
            let way = way(e).filter(|_| s.way);
            if !tags.is_empty() || e.via.is_some() || way.is_some() {
                ui.horizontal_wrapped(|ui| {
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
                        let way = RichText::new(way).size(t.text_small).color(t.muted);
                        ui.add(egui::Label::new(way).selectable(false));
                    }
                });
            }
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
    let fill = t.ink.gamma_multiply(0.035 + 0.025 * lift);
    ui.painter()
        .set(bg, Shape::rect_filled(rect, t.radius_lg, fill));
    r
}

/// A quiet icon button exactly `size`, so rows can count on its width.
pub fn icon(ui: &mut Ui, icon: Icon, label: &str, size: Vec2) -> Response {
    let t = Tokens::get(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let lit = ui.ctx().animate_bool_with_time(r.id, r.hovered(), 0.1);
    if lit > 0.0 {
        let spot = Rect::from_center_size(rect.center(), Vec2::splat(size.min_elem()));
        ui.painter()
            .rect_filled(spot, t.radius, t.hover.gamma_multiply(lit));
    }
    if r.hovered() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let ink = t.muted.lerp_to_gamma(t.ink, lit);
    let spot = Rect::from_center_size(rect.center(), Vec2::splat(15.0));
    icon.image(ui, 15.0).tint(ink).paint_at(ui, spot);
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    r.on_hover_text(label)
}

/// An icon in a soft disc of the accent, as empty states lead.
pub fn disc(ui: &mut Ui, icon: Icon, size: f32, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), size / 2.0, t.accent_soft);
    let glyph = (size * 0.46).round();
    let spot = Rect::from_center_size(rect.center(), Vec2::splat(glyph));
    icon.image(ui, glyph).tint(t.accent).paint_at(ui, spot);
}
