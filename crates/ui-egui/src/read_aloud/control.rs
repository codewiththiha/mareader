//! The compact player: steps, play, a seek track with time left, and stop.

use super::rate_label;
use crate::icons::anim::{self, Act as IconAct};
use crate::theme::{self, Tokens};
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::{icons::Icon, motion, widgets};
use egui::{
    Color32, CornerRadius, Id, LayerId, Order, Pos2, Rect, RichText, Sense, Shape, Stroke, Ui,
    UiBuilder, pos2, vec2,
};
use mareader_audio::RATES;
use mareader_engine::{
    command::Command,
    read_aloud::parts::Kind,
    read_aloud::player::{Act, View},
};
use mareader_settings::{Highlight, Player, ReadAloudSettings};

/// The pill's height, and its buttons'.
pub(super) const HEIGHT: f32 = 44.0;
const BUTTON: f32 = 32.0;
const PLAY: f32 = 36.0;
const ICON: f32 = 16.0;
const PAD: f32 = 6.0;
const TRACK_MOST: f32 = 168.0;
const TRACK_LEAST: f32 = 56.0;
/// The track shrinks to this before speed goes.
const TRACK_EASY: f32 = 110.0;
const TIME_W: f32 = 66.0;
const SPEED_W: f32 = 54.0;

/// Which parts fit: the track always, the time and speed when room allows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fit {
    pub track: f32,
    pub time: bool,
    pub speed: bool,
}

impl Fit {
    /// The parts that fit `room`, widest first.
    pub fn new(room: f32, escape: bool) -> Self {
        // Speed goes before the track is short; the time, only after.
        let tries = [(true, TRACK_EASY), (false, TRACK_LEAST)];
        let mut fit = Self {
            track: TRACK_LEAST,
            time: false,
            speed: false,
        };
        for (speed, least) in tries {
            fit = Self {
                track: 0.0,
                time: true,
                speed,
            };
            let track = room - fit.width(escape);
            fit.track = track.clamp(least, TRACK_MOST);
            if track >= least {
                return fit;
            }
        }
        fit.time = false;
        fit.track =
            (room - Self { track: 0.0, ..fit }.width(escape)).clamp(TRACK_LEAST, TRACK_MOST);
        fit
    }

    /// Gives the track what `room` has beyond the parts, when set wider.
    pub fn fill(mut self, room: f32, escape: bool) -> Self {
        self.track += (room - self.width(escape)).max(0.0);
        self
    }

    fn extra(&self) -> f32 {
        let time = if self.time { TIME_W + 8.0 } else { 0.0 };
        time + if self.speed { SPEED_W } else { 0.0 }
    }

    pub fn width(&self, escape: bool) -> f32 {
        // Pads, steps, play, card, more and stop, the rule and spacing.
        let fixed = 2.0 * PAD + 2.0 * BUTTON + PLAY + 3.0 * BUTTON + 22.0;
        fixed + self.track + self.extra() + if escape { BUTTON } else { 0.0 }
    }
}

/// The player's row, `card` of the way open; what it was asked.
pub(super) fn show(
    ui: &mut Ui,
    rect: Rect,
    (view, fit, card): (&View<'_>, Fit, f32),
    settings: &mut ReadAloudSettings,
    book: Option<mareader_library::BookId>,
    out: &mut Commands,
) -> Option<Act> {
    let t = Tokens::get(ui.ctx());
    let mut act = None;
    let mut x = rect.left() + PAD;
    let mut slot = |w: f32| {
        let r = Rect::from_min_size(pos2(x, rect.top()), vec2(w, HEIGHT));
        x += w;
        r
    };
    let unit = view.unit;
    let back = slot(BUTTON);
    if icon_button(
        ui,
        back,
        Icon::SkipBack,
        &format!("Previous {unit}"),
        view.back,
    ) {
        act = Some(Act::Back);
    }
    if play_button(ui, slot(PLAY), view) {
        act = Some(Act::Toggle);
    }
    let next = slot(BUTTON);
    if icon_button(
        ui,
        next,
        Icon::SkipForward,
        &format!("Next {unit}"),
        view.next,
    ) {
        act = Some(Act::Next);
    }
    if let Some(kind) = view.escape {
        let r = slot(BUTTON);
        let label = format!("Skip the rest of this {}", kind.name().to_lowercase());
        if icon_button(ui, r, Icon::LeaveTable, &label, true) {
            act = Some(Act::Escape);
        }
    }
    slot(10.0);
    let time_w = if fit.time { TIME_W + 8.0 } else { 0.0 };
    let middle = slot(fit.track + time_w);
    match &view.note {
        Some((text, bad)) => {
            // A voice that won't speak is fixed in its settings.
            if note(ui, middle, text, *bad, *bad && !view.narrated) {
                let tab = mareader_engine::settings::Tab::ReadAloud;
                out.push(Sent::Engine(Command::OpenSettings(tab)));
            }
        }
        None => {
            let (track, time) = middle.split_left_right_at_x(middle.left() + fit.track);
            if let Some(share) = seek_track(ui, track, view) {
                act = Some(Act::Seek(share));
            }
            if fit.time {
                time_label(ui, time.with_min_x(time.left() + 8.0), view);
            }
        }
    }
    slot(4.0);
    if fit.speed {
        speed(ui, slot(SPEED_W), settings, out);
    }
    if let Some(to) = card_button(ui, slot(BUTTON), card) {
        act = Some(Act::Card(to));
    }
    options(ui, slot(BUTTON), settings, view.narrated, book, out);
    let rule = slot(8.0);
    let half = vec2(0.0, BUTTON * 0.28);
    let stroke = Stroke::new(1.0, t.line);
    (ui.painter()).line_segment([rule.center() - half, rule.center() + half], stroke);
    if icon_button(ui, slot(BUTTON), Icon::Close, "Stop reading aloud", true) {
        act = Some(Act::Stop);
    }
    act
}

/// Sentence steps animate; the other player controls use static marks.
fn step_icon(icon: Icon) -> bool {
    matches!(icon, Icon::SkipBack | Icon::SkipForward)
}

/// A round, frameless icon button; true when clicked.
fn icon_button(ui: &mut Ui, slot: Rect, icon: Icon, label: &str, enabled: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let rect = Rect::from_center_size(slot.center(), egui::Vec2::splat(BUTTON));
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with(("read-aloud", label)), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label));
    let time = motion::secs(ui.ctx(), 0.12);
    let hot = ui
        .ctx()
        .animate_bool_with_time(r.id, enabled && r.hovered(), time);
    let fill = if r.is_pointer_button_down_on() {
        t.pressed
    } else {
        t.hover
    };
    (ui.painter()).circle_filled(rect.center(), BUTTON / 2.0, fill.gamma_multiply(hot));
    let ink = match enabled {
        true => t.ink,
        false => t.muted.gamma_multiply(0.5),
    };
    let mark = Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON));
    if step_icon(icon) {
        let _act = anim::announce(ui, IconAct::Click);
        anim::paint(ui, mark, icon, ink, label, &r);
    } else {
        icon.paint(ui, mark, ink);
    }
    widgets::tip(&r, label, None);
    enabled && r.clicked()
}

/// A chevron that turns as the card opens; the way asked, when clicked.
fn card_button(ui: &mut Ui, slot: Rect, card: f32) -> Option<bool> {
    let t = Tokens::get(ui.ctx());
    let open = card > 0.5;
    let label = if open {
        "Collapse player"
    } else {
        "Expand player"
    };
    let rect = Rect::from_center_size(slot.center(), egui::Vec2::splat(BUTTON));
    let r = ui.interact(rect, ui.id().with("read-aloud-card"), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    let hot = (ui.ctx()).animate_bool_with_time(r.id, r.hovered(), motion::secs(ui.ctx(), 0.12));
    (ui.painter()).circle_filled(rect.center(), BUTTON / 2.0, t.hover.gamma_multiply(hot));
    let mark = Rect::from_center_size(rect.center(), egui::Vec2::splat(ICON));
    let turn = std::f32::consts::PI * card;
    (Icon::ChevronUp.image(ui, ICON))
        .tint(t.ink)
        .rotate(turn, egui::Vec2::splat(0.5))
        .paint_at(ui, mark);
    widgets::tip(&r, label, None);
    r.clicked().then_some(!open)
}

/// The accent circle whose triangle folds into two bars; true when clicked.
pub(super) fn play_button(ui: &mut Ui, slot: Rect, view: &View<'_>) -> bool {
    let t = Tokens::get(ui.ctx());
    let label = if view.playing { "Pause" } else { "Play" };
    let rect = Rect::from_center_size(slot.center(), egui::Vec2::splat(PLAY));
    let sense = if view.ready {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with("read-aloud-play"), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, view.ready, label));
    let ctx = ui.ctx().clone();
    let quick = motion::secs(&ctx, 0.1);
    let hot = ctx.animate_bool_with_time(r.id.with("hot"), r.hovered(), quick);
    let down = ctx.animate_bool_with_time(r.id.with("down"), r.is_pointer_button_down_on(), quick);
    let morph =
        ctx.animate_bool_with_time(r.id.with("morph"), view.playing, motion::secs(&ctx, 0.18));
    let scale = 1.0 + 0.06 * hot - 0.08 * down;
    let radius = PLAY / 2.0 * scale;
    let fill = if view.ready {
        t.accent
    } else {
        t.accent.gamma_multiply(0.45)
    };
    ui.painter().circle_filled(rect.center(), radius, fill);
    let ink = theme::ink_on(t.accent);
    for quad in morph_quads(rect.center(), 13.0 * scale, ease(morph)) {
        ui.painter()
            .add(Shape::convex_polygon(quad.to_vec(), ink, Stroke::NONE));
    }
    if view.loading {
        spinner(ui, rect.center(), radius + 3.5, t.accent);
    }
    widgets::tip(&r, label, None);
    view.ready && r.clicked()
}

/// Play's triangle in two halves at 0, pause's two bars at 1.
fn morph_quads(c: Pos2, size: f32, k: f32) -> [[Pos2; 4]; 2] {
    let h = size;
    let (top, bottom) = (c.y - h / 2.0, c.y + h / 2.0);
    // The triangle sits a little right, so it looks centred.
    let (l, r) = (c.x - h * 0.42, c.x + h * 0.58);
    let m = (l + r) / 2.0;
    let play = [
        [
            pos2(l, top),
            pos2(m, top + h / 4.0),
            pos2(m, bottom - h / 4.0),
            pos2(l, bottom),
        ],
        [
            pos2(m, top + h / 4.0),
            pos2(r, c.y),
            pos2(r, c.y),
            pos2(m, bottom - h / 4.0),
        ],
    ];
    let (bl, br, bar) = (c.x - h * 0.4, c.x + h * 0.4, h * 0.3);
    let pause = [
        [
            pos2(bl, top),
            pos2(bl + bar, top),
            pos2(bl + bar, bottom),
            pos2(bl, bottom),
        ],
        [
            pos2(br - bar, top),
            pos2(br, top),
            pos2(br, bottom),
            pos2(br - bar, bottom),
        ],
    ];
    let mut out = play;
    for (o, p) in out.iter_mut().flatten().zip(pause.iter().flatten()) {
        *o = o.lerp(*p, k);
    }
    out
}

/// Ease in and out, so the fold starts and lands softly.
fn ease(k: f32) -> f32 {
    k * k * (3.0 - 2.0 * k)
}

/// A quarter arc running round `c`, while the voice loads.
fn spinner(ui: &Ui, c: Pos2, radius: f32, colour: Color32) {
    let time = ui.input(|i| i.time) as f32;
    let start = time * 5.0;
    let points: Vec<Pos2> = (0..=20)
        .map(|i| start + i as f32 / 20.0 * std::f32::consts::FRAC_PI_2 * 1.4)
        .map(|a| c + radius * vec2(a.cos(), a.sin()))
        .collect();
    ui.painter()
        .add(Shape::line(points, Stroke::new(2.0, colour)));
    ui.ctx().request_repaint();
}

/// The track read along; the share to read from once let go or clicked.
fn seek_track(ui: &mut Ui, rect: Rect, view: &View<'_>) -> Option<f32> {
    let t = Tokens::get(ui.ctx());
    let ctx = ui.ctx().clone();
    let id = ui.id().with("read-aloud-seek");
    let sense = if view.seek {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, id, sense);
    let rail = rect.shrink2(vec2(6.0, 0.0));
    let share_at = |x: f32| ((x - rail.left()) / rail.width().max(1.0)).clamp(0.0, 1.0);
    let pointer = r
        .hover_pos()
        .or(r.interact_pointer_pos())
        .map(|p| share_at(p.x));
    let dragging = r.dragged() && view.seek;
    let read = view.at.unwrap_or(0.0);
    let shown = match (dragging, pointer) {
        (true, Some(p)) => p,
        _ => ctx.animate_value_with_time(id.with("at"), read, motion::secs(&ctx, 0.25)),
    };
    let value = f64::from(shown) * 100.0;
    r.widget_info(|| {
        let mut info =
            egui::WidgetInfo::labeled(egui::WidgetType::Slider, view.seek, "Reading position");
        info.value = Some(value);
        info
    });
    let quick = motion::secs(&ctx, 0.12);
    let hot = ctx.animate_bool_with_time(
        id.with("hot"),
        view.seek && (r.hovered() || dragging),
        quick,
    );
    let thick = 4.0 + 2.0 * hot;
    let bar = Rect::from_center_size(rail.center(), vec2(rail.width(), thick));
    let round = CornerRadius::same(3);
    ui.painter().rect_filled(bar, round, t.track);
    // Hovered ahead or behind, the stretch to the pointer shows softly.
    if let (false, true, Some(p)) = (dragging, view.seek && r.hovered(), pointer) {
        let (a, b) = (shown.min(p), shown.max(p));
        let ghost = bar.with_min_x(bar.left() + bar.width() * a);
        let ghost = ghost.with_max_x(bar.left() + bar.width() * b);
        ui.painter()
            .rect_filled(ghost, round, t.accent.gamma_multiply(0.35));
    }
    let filled = bar.with_max_x(bar.left() + bar.width() * shown);
    ui.painter().rect_filled(filled, round, t.accent);
    let knob = pos2(bar.left() + bar.width() * shown, bar.center().y);
    let size = 2.5 + 3.5 * hot;
    ui.painter()
        .circle(knob, size, t.accent, Stroke::new(2.0, t.surface));
    if let (true, Some(share)) = (view.seek && (r.hovered() || dragging), pointer) {
        bubble(ui, rect, share, view);
    }
    let let_go = r.drag_stopped() || r.clicked();
    let share = r.interact_pointer_pos().map(|p| share_at(p.x));
    share.filter(|_| let_go && view.seek)
}

/// Above the track at `share`: where it lands and the time left from there.
fn bubble(ui: &Ui, track: Rect, share: f32, view: &View<'_>) {
    let t = Tokens::get(ui.ctx());
    let mut text = (view.hint)(share).unwrap_or_default();
    if let Some(left) = view.total.map(|all| all * f64::from(1.0 - share)) {
        let sep = if text.is_empty() { "" } else { " · " };
        text = format!("{text}{sep}{}", left_text(left));
    }
    if text.is_empty() {
        return;
    }
    let painter = ui
        .ctx()
        .layer_painter(LayerId::new(Order::Tooltip, Id::new("read-aloud-bubble")));
    let galley = painter.layout_no_wrap(text, theme::medium(t.text_small), t.ink);
    let size = galley.size() + vec2(16.0, 8.0);
    let x = track.left() + 6.0 + (track.width() - 12.0) * share;
    let mut at = Rect::from_center_size(pos2(x, track.top() - 8.0 - size.y / 2.0), size);
    let screen = ui.ctx().content_rect().shrink(4.0);
    at = at.translate(vec2(
        (screen.left() - at.left()).max(0.0) + (screen.right() - at.right()).min(0.0),
        0.0,
    ));
    let round = CornerRadius::same(t.radius as u8);
    painter.rect_filled(at, round, t.surface);
    (painter).rect_stroke(
        at,
        round,
        Stroke::new(1.0, t.line),
        egui::StrokeKind::Inside,
    );
    painter.galley(at.center() - galley.size() / 2.0, galley, t.ink);
}

/// The time left, or the share read, on one short line; a click swaps.
fn time_label(ui: &mut Ui, rect: Rect, view: &View<'_>) {
    let t = Tokens::get(ui.ctx());
    let id = Id::new("read-aloud-time-shows-share");
    let share = ui
        .data_mut(|d| d.get_persisted::<bool>(id))
        .unwrap_or(false);
    let (text, said) = match (share, view.at, view.left) {
        (true, Some(at), _) => (
            format!("{:.0}%", at * 100.0),
            format!("{:.0}% read", at * 100.0),
        ),
        (false, _, Some(left)) => (left_short(left), left_text(left)),
        _ => ("–".to_owned(), "Not timed yet".to_owned()),
    };
    let said = if view.loading {
        format!("{said} · loading voice")
    } else {
        said
    };
    let r = ui.interact(rect, ui.id().with("read-aloud-time"), Sense::click());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &said));
    let quick = motion::secs(ui.ctx(), 0.12);
    let hot = (ui.ctx()).animate_bool_with_time(r.id, r.hovered(), quick);
    let ink = t.muted.lerp_to_gamma(t.ink, hot);
    let font = egui::FontId::proportional(t.text_small);
    let galley = ui.painter().layout_no_wrap(text, font, ink);
    // The mark says what the number is; it goes first when room is short.
    let mark = 11.0;
    let gap = 4.0;
    let full = mark + gap + galley.size().x;
    let marked = full <= rect.width() - 4.0;
    let width = if marked { full } else { galley.size().x };
    // Pinned to the track's end, so it stays put as the digits change.
    let left = rect.left() + 2.0;
    let pill = Rect::from_min_size(
        pos2(left - 6.0, rect.center().y - 11.0),
        vec2(width + 12.0, 22.0),
    );
    (ui.painter()).rect_filled(pill, CornerRadius::same(255), t.hover.gamma_multiply(hot));
    let mut x = left;
    if marked {
        let at = Rect::from_center_size(pos2(x + mark / 2.0, rect.center().y), vec2(mark, mark));
        match (share, view.at) {
            (true, Some(at_share)) => ring(ui, at, at_share, ink, t.track),
            _ => Icon::Clock.paint(ui, at, ink),
        }
        x += mark + gap;
    }
    let top = rect.center().y - galley.size().y / 2.0;
    (ui.painter().with_clip_rect(rect)).galley(pos2(x, top), galley, ink);
    let (tip, swap) = match share {
        true => (said, "Click for the time left"),
        false => (said, "Click for how much is read"),
    };
    widgets::tip(&r, &tip, Some(swap));
    if r.clicked() {
        ui.data_mut(|d| d.insert_persisted(id, !share));
    }
}

/// A small ring filled round to `share`, for how much is read.
fn ring(ui: &Ui, rect: Rect, share: f32, ink: Color32, track: Color32) {
    let c = rect.center();
    let radius = rect.width() / 2.0 - 1.0;
    ui.painter()
        .circle_stroke(c, radius, Stroke::new(1.5, track));
    let turn = std::f32::consts::TAU * share.clamp(0.0, 1.0);
    let steps = (share * 32.0).ceil().max(1.0) as usize;
    let start = -std::f32::consts::FRAC_PI_2;
    let points: Vec<Pos2> = (0..=steps)
        .map(|i| start + turn * i as f32 / steps as f32)
        .map(|a| c + radius * vec2(a.cos(), a.sin()))
        .collect();
    ui.painter().add(Shape::line(points, Stroke::new(1.5, ink)));
}

/// A fault or the end, where the track was; true when asked to fix it.
fn note(ui: &mut Ui, rect: Rect, text: &str, bad: bool, fixable: bool) -> bool {
    let t = Tokens::get(ui.ctx());
    let rect = rect.shrink2(vec2(4.0, 0.0));
    let colour = if bad { t.danger } else { t.muted };
    let font = egui::FontId::proportional(t.text_small);
    let shown = headline(text);
    let link = "Voice settings";
    let name = match fixable {
        true => format!("{shown} · {link}"),
        false => shown.clone(),
    };
    let sense = if fixable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let r = ui.interact(rect, ui.id().with("read-aloud-note"), sense);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, fixable, &name));
    let painter = ui.painter().with_clip_rect(rect);
    // The way to fix it stays whole; the fault gives way.
    let link = fixable.then(|| painter.layout_no_wrap(link.to_owned(), font.clone(), t.accent));
    let dot = painter.layout_no_wrap(" · ".to_owned(), font.clone(), t.muted);
    let tail = link.as_ref().map_or(0.0, |l| l.size().x + dot.size().x);
    let room = rect.width() - tail;
    let words = (link.is_none() || room >= 48.0).then(|| {
        let mut job = egui::text::LayoutJob::single_section(
            shown.clone(),
            egui::TextFormat::simple(font.clone(), colour),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(room.max(0.0));
        painter.layout_job(job)
    });
    let lead = words.as_ref().map_or(0.0, |w| w.size().x);
    let gap = if words.is_some() { dot.size().x } else { 0.0 };
    let width = lead + link.as_ref().map_or(0.0, |l| l.size().x + gap);
    let mut x = rect.center().x - width.min(rect.width()) / 2.0;
    let y = |h: f32| rect.center().y - h / 2.0;
    if let Some(w) = words {
        painter.galley(pos2(x, y(w.size().y)), w, colour);
        x += lead;
    }
    if let Some(l) = link {
        if gap > 0.0 {
            painter.galley(pos2(x, y(dot.size().y)), dot, t.muted);
            x += gap;
        }
        let under = Rect::from_min_size(pos2(x, y(l.size().y)), l.size());
        if r.hovered() {
            let base = under.bottom() - 1.0;
            let line = [pos2(under.left(), base), pos2(under.right(), base)];
            painter.line_segment(line, Stroke::new(1.0, t.accent));
        }
        painter.galley(under.min, l, t.accent);
    }
    let r = if fixable {
        r.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        r
    };
    widgets::tip(&r, text, fixable.then_some("Click to fix it"));
    fixable && r.clicked()
}

/// A fault's gist: before any detail, and starting with a capital.
fn headline(text: &str) -> String {
    let gist = text.split(": ").next().unwrap_or(text).trim();
    let mut chars = gist.chars();
    chars
        .next()
        .map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

/// "29m", "1h 44m" or "<1m": the time left, as short as it reads.
pub(super) fn left_short(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 if seconds < 1.0 => "0m".to_owned(),
        0 => "<1m".to_owned(),
        1..=59 => format!("{minutes}m"),
        _ if minutes.is_multiple_of(60) || minutes >= 600 => format!("{}h", minutes / 60),
        _ => format!("{}h {}m", minutes / 60, minutes % 60),
    }
}

/// "12 min left", rounded so it doesn't flicker.
pub(super) fn left_text(seconds: f64) -> String {
    let minutes = (seconds / 60.0).round() as u64;
    match minutes {
        0 if seconds < 1.0 => "No time left".to_owned(),
        0 => "Under a minute left".to_owned(),
        1..=59 => format!("{minutes} min left"),
        _ if minutes.is_multiple_of(60) => format!("{} h left", minutes / 60),
        _ => format!("{} h {} min left", minutes / 60, minutes % 60),
    }
}

/// The speed picker: the rates offered, the one in use shown.
fn speed(ui: &mut Ui, slot: Rect, settings: &mut ReadAloudSettings, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let text = RichText::new(rate_label(settings.rate)).size(t.text_small);
    let button = egui::Button::new(text)
        .frame_when_inactive(false)
        .corner_radius(CornerRadius::same(255))
        .min_size(vec2(SPEED_W - 4.0, 26.0));
    let chip = slot.shrink2(vec2(2.0, (HEIGHT - 26.0) / 2.0));
    // A faint ring says the speed is a control, not a label.
    let ring = Stroke::new(1.0, t.line);
    (ui.painter()).rect_stroke(
        chip,
        CornerRadius::same(255),
        ring,
        egui::StrokeKind::Inside,
    );
    // Centred and filling the ring, so the rate sits in its middle.
    let middle = egui::Layout::centered_and_justified(egui::Direction::LeftToRight);
    let place = UiBuilder::new().max_rect(chip).layout(middle);
    ui.scope_builder(place, |ui| {
        // "1.25×" stays on one line, whatever the room.
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
        let r = menu(ui, button, SPEED_MENU, |ui| {
            for rate in RATES {
                let on = (settings.rate - rate).abs() < 0.01;
                if ui.selectable_label(on, rate_label(rate)).clicked() {
                    if !on {
                        settings.rate = rate;
                        out.push(Sent::Ui(UiCommand::Notify(
                            mareader_engine::toast::Toast::info(format!(
                                "Speed {}",
                                rate_label(rate)
                            ))
                            .player(),
                        )));
                    }
                    ui.close();
                }
            }
        });
        widgets::tip(&r, "Speed", None);
    });
}

/// The options behind More: what is skipped and lit, then voice settings.
fn options(
    ui: &mut Ui,
    slot: Rect,
    s: &mut ReadAloudSettings,
    narrated: bool,
    book: Option<mareader_library::BookId>,
    out: &mut Commands,
) {
    let more = egui::Button::image(Icon::More.image(ui, ICON))
        .image_tint_follows_text_color(true)
        .frame_when_inactive(false)
        .corner_radius(CornerRadius::same(255))
        .min_size(egui::Vec2::splat(BUTTON));
    let place = UiBuilder::new().max_rect(Rect::from_center_size(
        slot.center(),
        egui::Vec2::splat(BUTTON),
    ));
    ui.scope_builder(place, |ui| {
        let r = menu(ui, more, OPTIONS_MENU, |ui| {
            choices(ui, s, narrated, book, out)
        });
        let name = "Read-aloud options";
        r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, name));
        widgets::tip(&r, name, None);
    });
}

/// The speed and options menus' widths.
const SPEED_MENU: f32 = 96.0;
const OPTIONS_MENU: f32 = 264.0;

/// A button's menu, kept above the player it opens from.
fn menu(
    ui: &mut Ui,
    button: egui::Button<'_>,
    width: f32,
    add: impl FnOnce(&mut Ui),
) -> egui::Response {
    let r = ui.add(button);
    let popup = egui::Popup::menu(&r)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .gap(6.0)
        .width(width);
    let layer = LayerId::new(Order::Foreground, popup.get_id());
    popup.show(|ui| {
        ui.set_width(width);
        add(ui);
    });
    // A press raises the player over it until the release closes it.
    if egui::Popup::is_id_open(ui.ctx(), layer.id) {
        ui.ctx().move_to_top(layer);
    }
    r
}

/// What is skipped, how it's lit, the player's size, then voice settings.
fn choices(
    ui: &mut Ui,
    s: &mut ReadAloudSettings,
    narrated: bool,
    book: Option<mareader_library::BookId>,
    out: &mut Commands,
) {
    ui.spacing_mut().item_spacing.y = 2.0;
    if narrated {
        widgets::menu_switch(ui, Icon::Note, "Skip notes", &mut s.skip_notes);
    }
    let skip = &mut s.skip_page_numbers;
    widgets::menu_switch(ui, Icon::PageNumber, "Skip page numbers", skip);
    widgets::menu_switch(ui, Icon::Follow, "Turn pages to follow", &mut s.follow);
    if !narrated {
        heading(ui, "Highlight");
        let all = [Highlight::Sentence, Highlight::Phrases, Highlight::Words];
        widgets::segments(ui, &mut s.highlight, &all, 3, |h| {
            let label = match h {
                Highlight::Sentence => "Sentence",
                Highlight::Phrases => "Phrases",
                Highlight::Words => "Words",
            };
            (None, label)
        });
    }
    if let Some(book) = book
        && let Some(mut mask) = s.book_skips.get(&book).copied()
    {
        heading(ui, "Always skip in this book");
        for kind in Kind::ALL {
            if mask & kind.bit() == 0 {
                continue;
            }
            let mut on = true;
            let label = format!("Skip {}s", kind.name().to_lowercase());
            if widgets::menu_switch(ui, super::ask::sign(kind), &label, &mut on).changed() && !on {
                mask &= !kind.bit();
            }
        }
        if mask == 0 {
            s.book_skips.remove(&book);
        } else {
            s.book_skips.insert(book, mask);
        }
    }
    heading(ui, "Player");
    let all = [Player::Bar, Player::Compact, Player::Full];
    widgets::segments(ui, &mut s.player, &all, 3, |p| {
        let label = match p {
            Player::Bar => "Bar",
            Player::Compact => "Compact",
            Player::Full => "Full",
        };
        (None, label)
    });
    ui.add_space(6.0);
    ui.separator();
    let row = widgets::menu_row(ui, Icon::Settings, "Voice settings…", None, false);
    let t = Tokens::get(ui.ctx());
    let at = Rect::from_center_size(row.rect.right_center() - vec2(18.0, 0.0), vec2(14.0, 14.0));
    Icon::ChevronRight
        .image(ui, 14.0)
        .tint(t.muted)
        .paint_at(ui, at);
    if row.clicked() {
        out.push(Sent::Engine(Command::OpenSettings(
            mareader_engine::settings::Tab::ReadAloud,
        )));
        ui.close();
    }
}

/// A group's small name, a breath above it.
fn heading(ui: &mut Ui, text: &str) {
    ui.add_space(6.0);
    widgets::menu_note(ui, text);
    ui.add_space(2.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_player_steps_use_icon_runs() {
        assert!(step_icon(Icon::SkipBack));
        assert!(step_icon(Icon::SkipForward));
        assert!(!step_icon(Icon::Pause));
        assert!(!step_icon(Icon::Close));
    }

    #[test]
    fn time_left_reads_plainly_and_rounds() {
        assert_eq!(left_text(0.2), "No time left");
        assert_eq!(left_text(20.0), "Under a minute left");
        assert_eq!(left_text(100.0), "2 min left");
        assert_eq!(left_text(3600.0), "1 h left");
        assert_eq!(left_text(3600.0 + 65.0 * 60.0), "2 h 5 min left");
        assert_eq!(left_short(0.2), "0m");
        assert_eq!(left_short(20.0), "<1m");
        assert_eq!(left_short(29.0 * 60.0), "29m");
        assert_eq!(left_short(104.0 * 60.0), "1h 44m");
        assert_eq!(left_short(7200.0), "2h");
        assert_eq!(left_short(36_000.0 + 1800.0), "10h");
    }

    #[test]
    fn a_fault_shows_its_gist_and_keeps_details_for_the_tip() {
        let model = "couldn't load the voice model: /tmp/m/kokoro.safetensors";
        assert_eq!(headline(model), "Couldn't load the voice model");
        assert_eq!(
            headline("Kokoro isn't downloaded"),
            "Kokoro isn't downloaded"
        );
        assert_eq!(headline(""), "");
    }

    #[test]
    fn the_longest_short_time_fits_its_slot() {
        let ctx = egui::Context::default();
        let mut width = 0.0;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let font = egui::FontId::proportional(12.0);
            let text = left_short(9.0 * 3600.0 + 59.0 * 60.0);
            let galley = ui.painter().layout_no_wrap(text, font, Color32::WHITE);
            width = galley.size().x;
        });
        // No painter takes the font atlas here, so it's let go.
        out.textures_delta.clear();
        // The clock, its gap and the inset come before the text.
        assert!(width > 0.0 && 2.0 + 11.0 + 4.0 + width <= TIME_W, "{width}");
    }

    #[test]
    fn a_narrow_pane_drops_speed_then_time_then_shortens_the_track() {
        let wide = Fit::new(800.0, false);
        assert_eq!(
            (wide.track, wide.time, wide.speed),
            (TRACK_MOST, true, true)
        );
        let roomy = wide.width(false);
        let squeezed = Fit::new(roomy - 40.0, false);
        assert!(squeezed.speed && squeezed.time && squeezed.track < TRACK_MOST);
        // Speed goes before the track is short.
        let narrow = Fit::new(roomy - (TRACK_MOST - TRACK_EASY) - 10.0, false);
        assert!(!narrow.speed && narrow.time && narrow.track > TRACK_EASY);
        // Without the time, the track takes its room back.
        let none = Fit {
            track: 0.0,
            time: false,
            speed: false,
        };
        let bare = Fit::new(none.width(false) + TRACK_LEAST + TIME_W + 7.0, false);
        assert!(!bare.time && bare.track > TRACK_LEAST + TIME_W);
        let tiny = Fit::new(200.0, true);
        assert_eq!(
            (tiny.time, tiny.speed, tiny.track),
            (false, false, TRACK_LEAST)
        );
        assert!(Fit::new(roomy, true).width(true) <= roomy + 1.0);
    }

    #[test]
    fn the_play_triangle_folds_into_two_bars() {
        let c = pos2(0.0, 0.0);
        let play = morph_quads(c, 10.0, 0.0);
        let pause = morph_quads(c, 10.0, 1.0);
        // Play's tip meets at one point; pause's bars stand apart.
        assert_eq!(play[1][1], play[1][2]);
        assert!(pause[0][1].x < pause[1][0].x);
        assert!(ease(0.5) == 0.5 && ease(0.0) == 0.0 && ease(1.0) == 1.0);
    }
}
