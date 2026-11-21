//! The deck's flash card: it turns over in depth and swipes aside.
//!
//! Each face is painted as shapes and tessellated, then every vertex turns
//! about the card's upright axis and is seen in perspective, so the text
//! turns with the card. The cards still to come peek out beneath it.

use std::f32::consts::PI;
use std::sync::Arc;

use egui::emath::Rot2;
use egui::epaint::{Mesh, Tessellator};
use egui::text::{LayoutJob, TextFormat};
use egui::{
    Align, Color32, CursorIcon, FontId, Galley, Id, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2, pos2, vec2,
};

use crate::motion;
use crate::theme::{self, Tokens};
use crate::widgets;

/// What the front says to a screen reader, after the word.
pub const TURN_HINT: &str = "turn to see its meaning";
/// Seconds to turn over, and to fly aside.
const TURN: f32 = 0.45;
const FLY: f32 = 0.28;
/// A swipe: past this share of the width, or faster than this flick.
const SWIPE: f32 = 0.28;
const FLICK: f32 = 600.0;
/// Degrees of tilt at a full width's drag.
const TILT: f32 = 12.0;
/// The eye's distance, in card widths.
const DEPTH: f32 = 3.0;
/// How much of a drag past either end the card follows.
const BAND: f32 = 0.25;
/// How far each card beneath shows past the bottom, and how it shrinks.
const PEEK: f32 = 6.0;
const SHRINK: f32 = 0.04;
/// Room under the card for the ones beneath.
pub const UNDER: f32 = 2.0 * PEEK + 4.0;

/// One card's two faces.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Faces {
    pub word: String,
    /// The band, in the front's top right corner.
    pub badge: String,
    pub meaning: String,
    /// The word class, under the meaning.
    pub class: String,
    pub synonyms: String,
    pub usage: String,
}

impl Faces {
    /// What a screen reader hears: the word, or the back once turned.
    pub fn label(&self, turned: bool) -> String {
        if !turned {
            return format!("{}, {TURN_HINT}", self.word);
        }
        let back = [&self.meaning, &self.class, &self.synonyms, &self.usage];
        let back = back.into_iter().filter(|s| !s.is_empty());
        back.map(String::as_str).collect::<Vec<_>>().join(" · ")
    }
}

/// What the reader did to the card this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gesture {
    Turn,
    Step(isize),
}

/// Where a card stands: its middle, its turn and tilt, its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub center: Pos2,
    /// Radians about the upright axis; 0 faces the reader.
    pub angle: f32,
    pub tilt: f32,
    pub scale: f32,
    /// The eye's distance, in points.
    pub depth: f32,
}

impl Pose {
    /// Where a point `p` of the flat card, from its middle, is seen.
    pub fn place(&self, p: Vec2) -> Pos2 {
        let (sin, cos) = self.angle.sin_cos();
        let near = self.depth / (self.depth + p.x * sin).max(1.0);
        let flat = vec2(p.x * cos * near, p.y * near) * self.scale;
        self.center + Rot2::from_angle(self.tilt) * flat
    }
}

/// Where a drag past an end stops following the pointer.
pub fn band(dx: f32, at: usize, count: usize) -> f32 {
    let open = if dx < 0.0 { at + 1 < count } else { at > 0 };
    if open { dx } else { dx * BAND }
}

/// The step a drag let go of makes: on, back, or none.
pub fn settle(dx: f32, vx: f32, width: f32, at: usize, count: usize) -> isize {
    let far = dx.abs() > width * SWIPE;
    let flick = vx.abs() > FLICK && dx != 0.0 && vx.signum() == dx.signum();
    if !(far || flick) {
        return 0;
    }
    let step = if dx < 0.0 { 1 } else { -1 };
    let open = if step > 0 { at + 1 < count } else { at > 0 };
    if open { step } else { 0 }
}

fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A card leaving: which, how turned, and where it heads.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Fly {
    from: usize,
    turn: f32,
    /// -1 leaves left, 1 right.
    dir: f32,
    dx: f32,
    t: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct State {
    /// 0 shows the front, 1 the back, linear in time.
    turn: f32,
    /// The drag so far, before any band.
    dx: f32,
    shown: Option<usize>,
    fly: Option<Fly>,
}

/// The cards at `at` of `count`, `face` naming each; `size` is the top one's.
pub fn show(
    ui: &mut Ui,
    id: Id,
    size: Vec2,
    (at, count, turned): (usize, usize, bool),
    face: &dyn Fn(usize) -> Option<Faces>,
) -> Option<Gesture> {
    let ctx = ui.ctx().clone();
    let mut s: State = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    let dt = ui.input(|i| i.stable_dt).min(0.1);
    let (rect, resp) = ui.allocate_exact_size(size + vec2(0.0, UNDER), Sense::click_and_drag());
    let moving = motion::secs(&ctx, 1.0) > 0.0;
    s.follow(at, turned, moving);
    let target = f32::from(u8::from(turned));
    s.turn = match moving {
        true => toward(s.turn, target, dt / TURN),
        false => target,
    };
    let gesture = s.handle(ui, &resp, (at, count), dt, moving);
    if let Some(held) = ctx.data(|d| d.get_temp::<f32>(id.with("held"))) {
        s.dx = held;
    }
    s.fly = s.fly.and_then(|mut fly| {
        fly.t += dt / FLY;
        (fly.t < 1.0).then_some(fly)
    });
    let card = Rect::from_min_size(rect.min, size);
    s.draw(ui, card, (at, count), face);
    if s.turn != target || s.dx != 0.0 || s.fly.is_some() {
        ctx.request_repaint();
    }
    if resp.dragged() {
        ctx.set_cursor_icon(CursorIcon::Grabbing);
    } else if resp.hovered() {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    }
    let label = face(at).map(|f| f.label(turned)).unwrap_or_default();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    widgets::tip(&resp, "Click to turn, drag to move on", Some("Space · ← →"));
    s.shown = Some(at);
    ctx.data_mut(|d| d.insert_temp(id, s));
    gesture
}

/// Shots: holds the card `dx` aside, as a drag would.
pub fn hold(ctx: &egui::Context, id: Id, dx: f32) {
    ctx.data_mut(|d| d.insert_temp(id.with("held"), dx));
}

impl State {
    /// A new card shown sends the last one flying, toward where it left.
    fn follow(&mut self, at: usize, turned: bool, moving: bool) {
        let Some(from) = self.shown.filter(|&from| from != at) else {
            return;
        };
        let fly = Fly {
            from,
            turn: self.turn,
            dir: if at > from { -1.0 } else { 1.0 },
            dx: self.dx,
            t: 0.0,
        };
        self.fly = moving.then_some(fly);
        self.dx = 0.0;
        self.turn = f32::from(u8::from(turned));
    }

    /// A click turns the card, a drag let go of far enough moves on.
    fn handle(
        &mut self,
        ui: &Ui,
        resp: &egui::Response,
        (at, count): (usize, usize),
        dt: f32,
        moving: bool,
    ) -> Option<Gesture> {
        let mut gesture = None;
        if resp.dragged() {
            self.dx += resp.drag_delta().x;
        }
        if resp.drag_stopped() {
            let vx = ui.input(|i| i.pointer.velocity().x);
            let step = settle(self.dx, vx, resp.rect.width(), at, count);
            if step != 0 {
                gesture = Some(Gesture::Step(step));
            }
        }
        if resp.clicked() {
            gesture = Some(Gesture::Turn);
        }
        if !resp.dragged() && !matches!(gesture, Some(Gesture::Step(_))) {
            self.dx = spring(self.dx, dt, moving);
        }
        gesture
    }

    /// The cards beneath, the top one, and the one flying off over it.
    fn draw(
        &self,
        ui: &Ui,
        card: Rect,
        (at, count): (usize, usize),
        face: &dyn Fn(usize) -> Option<Faces>,
    ) {
        let size = card.size();
        let mut cards = Cards {
            painter: ui.painter().clone(),
            tess: tessellator(ui.ctx()),
            size,
            depth: size.x * DEPTH,
            t: Tokens::get(ui.ctx()),
        };
        let rise = self.fly.map_or(1.0, |fly| ease(fly.t));
        let under = count.saturating_sub(at.saturating_add(1)).min(2);
        for i in (1..=under).rev() {
            cards.blank(card, i as f32 + 1.0 - rise);
        }
        let dx = band(self.dx, at, count);
        if let Some(faces) = face(at) {
            let lift = 1.0 + 0.02 * (dx.abs() / 40.0).min(1.0);
            let level = 1.0 - rise;
            let pose = Pose {
                center: card.center() + vec2(dx, level * peek_drop(size.y, 1.0)),
                angle: ease(self.turn) * PI,
                tilt: dx / size.x * TILT.to_radians(),
                scale: (1.0 - SHRINK * level) * lift,
                depth: cards.depth,
            };
            cards.card(&faces, pose, 1.0);
        }
        if let Some(fly) = self.fly
            && let Some(faces) = face(fly.from)
        {
            let e = ease(fly.t);
            let off = fly.dx + (fly.dir * size.x * 1.3 - fly.dx) * e;
            let pose = Pose {
                center: card.center() + vec2(off, 0.0),
                angle: ease(fly.turn) * PI,
                tilt: off / size.x * TILT.to_radians(),
                scale: 1.0,
                depth: cards.depth,
            };
            cards.card(&faces, pose, 1.0 - e);
        }
    }
}

fn toward(from: f32, to: f32, by: f32) -> f32 {
    if from < to {
        (from + by).min(to)
    } else {
        (from - by).max(to)
    }
}

/// A drag let go of eases back to rest.
fn spring(dx: f32, dt: f32, moving: bool) -> f32 {
    let dx = dx * (1.0 - (dt * 14.0).min(1.0));
    if !moving || dx.abs() < 0.5 { 0.0 } else { dx }
}

/// How far a card `level` deep sits below the top one's middle.
fn peek_drop(height: f32, level: f32) -> f32 {
    height * SHRINK * level / 2.0 + PEEK * level
}

fn tessellator(ctx: &egui::Context) -> Tessellator {
    let options = ctx.tessellation_options(|o| *o);
    let atlas = ctx.fonts(|f| f.font_image_size());
    Tessellator::new(ctx.pixels_per_point(), options, atlas, Vec::new())
}

/// What a frame's cards are painted with.
struct Cards {
    painter: Painter,
    tess: Tessellator,
    size: Vec2,
    depth: f32,
    t: Tokens,
}

impl Cards {
    /// A card beneath, `level` deep, its face hidden.
    fn blank(&mut self, top: Rect, level: f32) {
        let pose = Pose {
            center: top.center() + vec2(0.0, peek_drop(self.size.y, level)),
            angle: 0.0,
            tilt: 0.0,
            scale: 1.0 - SHRINK * level,
            depth: self.depth,
        };
        let shapes = self.sheet(0.5);
        self.paint(shapes, pose, 1.0);
    }

    fn card(&mut self, faces: &Faces, pose: Pose, alpha: f32) {
        let back = pose.angle > PI / 2.0;
        let mut shapes = self.sheet(1.0);
        match back {
            true => self.back(faces, &mut shapes),
            false => self.front(faces, &mut shapes),
        }
        let angle = if back { pose.angle - PI } else { pose.angle };
        self.paint(shapes, Pose { angle, ..pose }, alpha);
    }

    fn local(&self) -> Rect {
        Rect::from_center_size(Pos2::ZERO, self.size)
    }

    /// The card itself: its shadow, paper and edge.
    fn sheet(&self, shadow: f32) -> Vec<Shape> {
        let r = self.local();
        let radius = self.t.radius_lg * 1.5;
        let shade = egui::Shadow {
            offset: [0, 10],
            blur: 28,
            spread: 0,
            color: self.t.shadow.gamma_multiply(shadow),
        };
        vec![
            Shape::Rect(shade.as_shape(r, radius)),
            Shape::rect_filled(r, radius, self.t.surface),
            Shape::rect_stroke(r, radius, Stroke::new(1.0, self.t.line), StrokeKind::Inside),
        ]
    }

    fn front(&self, faces: &Faces, out: &mut Vec<Shape>) {
        let r = self.local();
        let room = r.width() - 64.0;
        let mut word = self.text(&faces.word, theme::medium(46.0), self.t.ink, room);
        if word.rect.width() > room {
            let size = (46.0 * room / word.rect.width()).max(14.0);
            word = self.text(&faces.word, theme::medium(size), self.t.ink, room);
        }
        let at = pos2(0.0, -word.rect.height() / 2.0);
        out.push(centred(at, word, self.t.ink));
        self.badge(&faces.badge, out);
    }

    fn back(&self, faces: &Faces, out: &mut Vec<Shape>) {
        let r = self.local();
        let room = r.width() - 56.0;
        let t = &self.t;
        let small = FontId::proportional(t.text_small + 1.0);
        let lines = [
            (&faces.meaning, theme::medium(20.0), t.ink, false),
            (&faces.class, small.clone(), t.muted, false),
            (&faces.synonyms, small.clone(), t.muted, false),
            (&faces.usage, small, t.muted, true),
        ];
        let galleys: Vec<_> = (lines.into_iter())
            .filter(|(text, ..)| !text.is_empty())
            .map(|(text, font, color, italics)| {
                let g = self.styled(text, font, color, italics, room);
                (g, color)
            })
            .collect();
        let gap = 10.0;
        let tall: f32 = galleys.iter().map(|(g, _)| g.rect.height()).sum();
        let spaced = gap * galleys.len().saturating_sub(1) as f32;
        let mut y = -(tall + spaced) / 2.0;
        for (galley, color) in galleys {
            let height = galley.rect.height();
            out.push(centred(pos2(0.0, y), galley, color));
            y += height + gap;
        }
        self.badge(&faces.badge, out);
    }

    /// The band, small in the top right corner.
    fn badge(&self, badge: &str, out: &mut Vec<Shape>) {
        if badge.is_empty() {
            return;
        }
        let r = self.local();
        let font = theme::medium(self.t.text_small);
        let galley = self.text(badge, font, self.t.accent, f32::INFINITY);
        let pill = galley.rect.size() + vec2(14.0, 6.0);
        let corner = r.right_top() + vec2(-16.0 - pill.x, 16.0);
        let rect = Rect::from_min_size(corner, pill);
        out.push(Shape::rect_filled(rect, pill.y / 2.0, self.t.track));
        let at = pos2(rect.center().x, rect.top() + 3.0);
        out.push(centred(at, galley, self.t.accent));
    }

    fn text(&self, text: &str, font: FontId, color: Color32, width: f32) -> Arc<Galley> {
        self.styled(text, font, color, false, width)
    }

    fn styled(
        &self,
        text: &str,
        font_id: FontId,
        color: Color32,
        italics: bool,
        width: f32,
    ) -> Arc<Galley> {
        let format = TextFormat {
            font_id,
            color,
            italics,
            ..TextFormat::default()
        };
        let mut job = LayoutJob::single_section(text.to_owned(), format);
        job.wrap.max_width = width;
        job.halign = Align::Center;
        self.painter.layout_job(job)
    }

    /// Tessellates `shapes`, then places every vertex as `pose` sees it.
    fn paint(&mut self, shapes: Vec<Shape>, pose: Pose, alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let mut mesh = Mesh::default();
        for shape in shapes {
            self.tess.tessellate_shape(shape, &mut mesh);
        }
        for v in &mut mesh.vertices {
            v.pos = pose.place(v.pos.to_vec2());
            if alpha < 1.0 {
                v.color = v.color.gamma_multiply(alpha);
            }
        }
        self.painter.add(Shape::mesh(mesh));
    }
}

/// `galley` with its top middle at `top`.
fn centred(top: Pos2, galley: Arc<Galley>, color: Color32) -> Shape {
    let at = pos2(top.x - galley.rect.center().x, top.y - galley.rect.top());
    Shape::galley(at, galley, color)
}

#[cfg(test)]
#[path = "flip_card_tests.rs"]
mod tests;
