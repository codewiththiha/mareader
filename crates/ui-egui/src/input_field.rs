//! Shared text input styled as a recessed paper well with focus lift.

use egui::{
    Align, Color32, CornerRadius, CursorIcon, EventFilter, FontId, Frame, Id, KeyboardShortcut,
    Layout, Margin, Rect, Response, RichText, Sense, Shape, Stroke, StrokeKind, TextEdit, Ui,
    UiBuilder, Vec2, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::widgets;

const REGULAR_H: f32 = 36.0;
const LARGE_H: f32 = 42.0;
const COMPACT_H: f32 = 32.0;
const FOCUS_TIME: f32 = 0.18;
const HOVER_TIME: f32 = 0.12;
const CLEAR_LABEL: &str = "Clear search";

/// Outcome of drawing an [`InputField`].
pub struct InputResponse {
    pub response: Response,
    pub rect: Rect,
    pub cleared: bool,
}

impl InputResponse {
    /// True if the text changed through typing or clicking the clear button.
    pub fn changed(&self) -> bool {
        self.cleared || self.response.changed()
    }
}

/// Global text field with recessed fill, focus lift and ambient halo.
pub struct InputField<'a> {
    text: &'a mut String,
    id: Option<Id>,
    hint: String,
    label: Option<String>,
    icon: Option<Icon>,
    clearable: bool,
    password: bool,
    multiline: bool,
    rows: usize,
    lock_focus: bool,
    pill: bool,
    width: f32,
    height: f32,
    text_size: Option<f32>,
    text_color: Option<Color32>,
    char_limit: Option<usize>,
    return_key: Option<Option<KeyboardShortcut>>,
    event_filter: Option<EventFilter>,
}

/// Starts a regular single-line [`InputField`].
pub fn input(text: &mut String) -> InputField<'_> {
    InputField::new(text, false)
}

/// Starts a multi-line [`InputField`] sharing the same visual treatment.
pub fn input_multiline(text: &mut String) -> InputField<'_> {
    InputField::new(text, true)
}

impl<'a> InputField<'a> {
    fn new(text: &'a mut String, multiline: bool) -> Self {
        Self {
            text,
            id: None,
            hint: String::new(),
            label: None,
            icon: None,
            clearable: false,
            password: false,
            multiline,
            rows: 4,
            lock_focus: false,
            pill: false,
            width: f32::INFINITY,
            height: REGULAR_H,
            text_size: None,
            text_color: None,
            char_limit: None,
            return_key: None,
            event_filter: None,
        }
    }

    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = hint.into();
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    pub fn char_limit(mut self, limit: usize) -> Self {
        self.char_limit = Some(limit);
        self
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: f32) -> Self {
        self.height = height;
        self
    }

    pub fn large(mut self) -> Self {
        self.height = LARGE_H;
        self
    }

    pub fn compact(mut self) -> Self {
        self.height = COMPACT_H;
        self
    }

    pub fn pill(mut self) -> Self {
        self.pill = true;
        self
    }

    pub fn text_size(mut self, size: f32) -> Self {
        self.text_size = Some(size);
        self
    }

    pub fn text_color(mut self, color: Color32) -> Self {
        self.text_color = Some(color);
        self
    }

    pub fn return_key(mut self, key: Option<KeyboardShortcut>) -> Self {
        self.return_key = Some(key);
        self
    }

    pub fn event_filter(mut self, filter: EventFilter) -> Self {
        self.event_filter = Some(filter);
        self
    }

    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    pub fn lock_focus(mut self, lock: bool) -> Self {
        self.lock_focus = lock;
        self
    }

    /// Draws the field and returns its response, rect and clear flag.
    pub fn show(self, ui: &mut Ui) -> InputResponse {
        if self.multiline {
            self.show_multiline(ui)
        } else {
            self.show_singleline(ui)
        }
    }

    fn show_singleline(self, ui: &mut Ui) -> InputResponse {
        let t = Tokens::get(ui.ctx());
        let ctx = ui.ctx().clone();
        let id = self.id.unwrap_or_else(|| ui.next_auto_id().with("input"));
        let w = if self.width.is_finite() {
            self.width.min(ui.available_width()).max(40.0)
        } else {
            ui.available_width().max(40.0)
        };
        let (rect, box_resp) = ui.allocate_exact_size(vec2(w, self.height), Sense::click());
        let focused = ui.memory(|m| m.has_focus(id));
        let hovered = box_resp.hovered() || ui.rect_contains_pointer(rect);
        let (lit, hov) = animate_state(&ctx, id, focused, hovered);
        let corner = corner_radius(self.height, self.pill, &t);
        paint_well(ui, rect, corner, (lit, hov), &t);

        let text_size = self.text_size.unwrap_or(if self.height >= LARGE_H {
            t.text + 0.5
        } else {
            t.text
        });
        let pad_x = horizontal_pad(self.height, self.pill);
        let left = paint_leading_icon(ui, rect, self.icon, (pad_x, text_size, lit), &t);
        let has_text = !self.text.is_empty();
        let btn = (self.height - 14.0).clamp(18.0, 24.0);
        let clear_shown = if self.clearable {
            let secs = crate::motion::secs(&ctx, FOCUS_TIME);
            ctx.animate_bool_with_time(id.with("clear"), has_text, secs)
        } else {
            0.0
        };
        let right = if self.clearable && has_text {
            rect.right() - 7.0 - btn - 4.0
        } else {
            rect.right() - pad_x
        };
        let room = Rect::from_min_max(
            pos2(left, rect.top() + 1.0),
            pos2(right.max(left + 20.0), rect.bottom() - 2.0),
        );
        let cleared = self.clearable
            && (has_text || clear_shown > 0.0)
            && paint_clear(ui, rect, (id, btn, clear_shown, has_text), &t);
        if cleared {
            self.text.clear();
        }
        let response = run_singleline(ui, self, id, text_size, room, &t);
        if box_resp.clicked() && !cleared {
            response.request_focus();
        }
        if cleared {
            response.request_focus();
            ctx.request_repaint();
        }
        if box_resp.hovered() && !response.hovered() {
            ctx.set_cursor_icon(CursorIcon::Text);
        }
        InputResponse {
            response,
            rect,
            cleared,
        }
    }

    fn show_multiline(self, ui: &mut Ui) -> InputResponse {
        let t = Tokens::get(ui.ctx());
        let ctx = ui.ctx().clone();
        let id = self.id.unwrap_or_else(|| ui.next_auto_id().with("input"));
        let focused = ui.memory(|m| m.has_focus(id));
        let prev = ui.data(|d| d.get_temp::<Rect>(id.with("rect")));
        let hovered = prev.is_some_and(|r| ui.rect_contains_pointer(r));
        let (lit, hov) = animate_state(&ctx, id, focused, hovered);
        let corner = (t.radius + 2.0).min(12.0);
        let hint = RichText::new(&self.hint).color(t.muted.gamma_multiply(0.75));
        let mut edit = TextEdit::multiline(self.text)
            .id(id)
            .hint_text(hint)
            .font(FontId::proportional(self.text_size.unwrap_or(t.text)))
            .text_color(self.text_color.unwrap_or(t.ink))
            .frame(Frame::NONE)
            .margin(Margin::ZERO)
            .desired_rows(self.rows)
            .desired_width(if self.width.is_finite() {
                self.width
            } else {
                f32::INFINITY
            })
            .lock_focus(self.lock_focus);
        if let Some(limit) = self.char_limit {
            edit = edit.char_limit(limit);
        }
        let halo_slot = ui.painter().add(Shape::Noop);
        let frame = Frame::new()
            .fill(well_fill(&t, lit, hov))
            .stroke(well_stroke(&t, lit, hov))
            .corner_radius(CornerRadius::same(corner as u8))
            .inner_margin(Margin::symmetric(12, 10));
        let shown = frame.show(ui, |ui| ui.add(edit));
        let rect = shown.response.rect;
        ui.data_mut(|d| d.insert_temp(id.with("rect"), rect));
        if lit > 0.01 {
            let halo_r = CornerRadius::same((corner + 3.0).clamp(0.0, 255.0) as u8);
            let glow = t.accent.gamma_multiply(0.11 * lit);
            let shape = Shape::rect_filled(rect.expand(3.0), halo_r, glow);
            ui.painter().set(halo_slot, shape);
        }
        if let Some(label) = &self.label {
            widgets::name(&shown.inner, label);
        }
        InputResponse {
            response: shown.inner,
            rect,
            cleared: false,
        }
    }
}

fn run_singleline(
    ui: &mut Ui,
    cfg: InputField<'_>,
    id: Id,
    text_size: f32,
    room: Rect,
    t: &Tokens,
) -> Response {
    let hint = RichText::new(cfg.hint).color(t.muted.gamma_multiply(0.75));
    let mut edit = TextEdit::singleline(cfg.text)
        .id(id)
        .hint_text(hint)
        .font(FontId::proportional(text_size))
        .text_color(cfg.text_color.unwrap_or(t.ink))
        .password(cfg.password)
        .frame(Frame::NONE)
        .margin(Margin::ZERO)
        .vertical_align(Align::Center)
        .min_size(room.size())
        .desired_width(room.width());
    if let Some(limit) = cfg.char_limit {
        edit = edit.char_limit(limit);
    }
    if let Some(key) = cfg.return_key {
        edit = edit.return_key(key);
    }
    if let Some(filter) = cfg.event_filter {
        edit = edit.event_filter(filter);
    }
    let builder = UiBuilder::new()
        .max_rect(room)
        .layout(Layout::left_to_right(Align::Center));
    let response = ui.scope_builder(builder, |ui| ui.add(edit)).inner;
    if let Some(label) = &cfg.label {
        widgets::name(&response, label);
    }
    response
}

fn horizontal_pad(height: f32, pill: bool) -> f32 {
    if pill {
        (height * 0.36).round()
    } else if height >= LARGE_H {
        14.0
    } else if height <= COMPACT_H {
        10.0
    } else {
        12.0
    }
}

fn ease_out(k: f32) -> f32 {
    1.0 - (1.0 - k.clamp(0.0, 1.0)).powi(3)
}

fn animate_state(ctx: &egui::Context, id: Id, focused: bool, hovered: bool) -> (f32, f32) {
    let focus_secs = crate::motion::secs(ctx, FOCUS_TIME);
    let hover_secs = crate::motion::secs(ctx, HOVER_TIME);
    let lit = ease_out(ctx.animate_bool_with_time(id.with("lit"), focused, focus_secs));
    let hov = ctx.animate_bool_with_time(id.with("hov"), hovered && !focused, hover_secs);
    (lit, hov)
}

fn corner_radius(height: f32, pill: bool, t: &Tokens) -> f32 {
    if pill {
        (height / 2.0).clamp(0.0, 255.0)
    } else if height >= LARGE_H {
        t.radius_lg
    } else if height <= COMPACT_H {
        t.radius + 1.0
    } else {
        t.radius + 2.0
    }
}

fn well_fill(t: &Tokens, lit: f32, hov: f32) -> Color32 {
    let idle = if t.dark {
        t.paper.lerp_to_gamma(Color32::BLACK, 0.24)
    } else {
        t.surface.lerp_to_gamma(t.paper, 0.55)
    };
    let hovered = idle.lerp_to_gamma(t.paper, 0.35);
    idle.lerp_to_gamma(hovered, hov).lerp_to_gamma(t.paper, lit)
}

fn well_stroke(t: &Tokens, lit: f32, hov: f32) -> Stroke {
    let idle = t.line.gamma_multiply(0.9);
    let hovered = idle.lerp_to_gamma(t.muted, 0.35);
    let color = idle
        .lerp_to_gamma(hovered, hov)
        .lerp_to_gamma(t.accent, 0.65 * lit);
    Stroke::new(1.0, color)
}

fn paint_well(ui: &Ui, rect: Rect, corner: f32, (lit, hov): (f32, f32), t: &Tokens) {
    let painter = ui.painter();
    if lit > 0.01 {
        let halo = rect.expand(3.0);
        let halo_r = CornerRadius::same((corner + 3.0).clamp(0.0, 255.0) as u8);
        painter.rect_filled(halo, halo_r, t.accent.gamma_multiply(0.11 * lit));
    }
    let fill = well_fill(t, lit, hov);
    let stroke = well_stroke(t, lit, hov);
    let radius = CornerRadius::same(corner.clamp(0.0, 255.0) as u8);
    painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
    // Subtle inner top recess shade melts away as focus lifts the surface.
    if lit < 0.99 {
        let alpha = if t.dark { 26 } else { 12 };
        let shade = Color32::from_black_alpha(alpha).gamma_multiply(1.0 - lit);
        let inset = corner.min(rect.width() * 0.25);
        painter.hline(
            (rect.left() + inset)..=(rect.right() - inset),
            rect.top() + 1.5,
            Stroke::new(1.0, shade),
        );
    }
}

fn paint_leading_icon(
    ui: &Ui,
    rect: Rect,
    icon: Option<Icon>,
    (pad_x, text_size, lit): (f32, f32, f32),
    t: &Tokens,
) -> f32 {
    let Some(icon) = icon else {
        return rect.left() + pad_x;
    };
    let glyph = (text_size + 1.5).clamp(14.0, 18.0);
    let mark = Rect::from_center_size(
        pos2(rect.left() + pad_x + glyph * 0.5, rect.center().y),
        Vec2::splat(glyph),
    );
    let tint = t.muted.lerp_to_gamma(t.accent, lit);
    icon.image(ui, glyph).tint(tint).paint_at(ui, mark);
    mark.right() + 9.0
}

fn paint_clear(
    ui: &mut Ui,
    rect: Rect,
    (id, button, shown, active): (Id, f32, f32, bool),
    t: &Tokens,
) -> bool {
    let centre = pos2(rect.right() - 7.0 - button * 0.5, rect.center().y);
    let spot = Rect::from_center_size(centre, Vec2::splat(button));
    let response = active.then(|| ui.interact(spot, id.with("clear-button"), Sense::click()));
    let hovered = response.as_ref().is_some_and(Response::hovered);
    let vis = if active { shown.max(0.01) } else { shown };
    let fill = match hovered {
        true => t.ink.gamma_multiply(0.14),
        false => t.ink.gamma_multiply(0.07),
    };
    let radius = button * 0.5 * (0.72 + 0.28 * vis);
    ui.painter()
        .circle_filled(centre, radius, fill.gamma_multiply(vis));
    let x = Rect::from_center_size(centre, Vec2::splat(12.0));
    let tint = t.muted.gamma_multiply(vis);
    Icon::Close.image(ui, 12.0).tint(tint).paint_at(ui, x);
    let Some(r) = response else {
        return false;
    };
    if hovered {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, CLEAR_LABEL));
    r.clicked()
}
