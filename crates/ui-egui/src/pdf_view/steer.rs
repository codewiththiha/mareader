//! Keyboard glide, held-arrow scrolling, and auto-scroll along strips.

use super::*;

/// A held arrow waits this long, then glides at this speed (mareader's).
const HOLD_DELAY: f64 = 0.35;
const HOLD_SPEED: f32 = 1000.0;
/// A slow frame moves a held glide no further than this one would.
const MAX_FRAME: f64 = 0.05;
/// Auto-scroll's pace along the strip, as mareader's 72 px a second.
pub const AUTO_SPEED: f32 = 72.0;

/// A tap moves one reading line: 8 % of the view, 40 to 80 points.
pub fn line_step(len: f32) -> f32 {
    (len * 0.08).clamp(40.0, 80.0)
}

/// A page key moves nearly a screen, keeping a sliver to hold the place.
pub fn screen_step(len: f32) -> f32 {
    (len * 0.9).max(1.0)
}

/// A key's glide along the strip. The view runs it, so quick presses add up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Glide {
    from: f32,
    to: f32,
    start: f64,
    secs: f64,
    /// Where it put the strip last frame.
    at: f32,
}

impl Glide {
    pub(super) fn new(from: f32, to: f32, now: f64, a: ScrollAnimation) -> Self {
        let secs = ((to - from).abs() / a.points_per_second).clamp(a.duration.min, a.duration.max);
        Self {
            from,
            to,
            start: now,
            secs: f64::from(secs),
            at: from,
        }
    }

    /// Moves the whole glide `d` along, as auto-scroll moves the strip.
    fn shift(&mut self, d: f32) {
        self.from += d;
        self.to += d;
        self.at += d;
    }

    /// This frame's position, and whether the glide is over.
    fn step(&mut self, now: f64) -> (f32, bool) {
        let t = ((now - self.start) / self.secs.max(1e-6)).clamp(0.0, 1.0) as f32;
        self.at = self.from + (self.to - self.from) * egui::emath::ease_in_ease_out(t);
        (self.at, t >= 1.0)
    }
}

/// This frame's move along the strip: the keys' and auto-scroll's together.
pub(super) fn keyed_scroll(
    view: &mut PdfView,
    at: &Place,
    ctx: &egui::Context,
    len: f32,
    now: f64,
) -> Option<f32> {
    let keyed = steered(view, at, ctx, len, now);
    let drift = drifted(view, at, ctx, now);
    // A key's glide rides along, or it would read the drift as a wheel.
    if let (Some(d), Some(g)) = (drift, &mut view.aim) {
        g.shift(d);
    }
    match (keyed, drift) {
        (None, None) => None,
        (a, b) => Some(a.unwrap_or(0.0) + b.unwrap_or(0.0)),
    }
}

/// Auto-scroll's step, at the same pace whatever the frame rate.
fn drifted(view: &mut PdfView, at: &Place, ctx: &egui::Context, now: f64) -> Option<f32> {
    let last = view.drift?;
    let size = view.layout.as_ref()?.size;
    let (here, end) = match at.mode {
        ViewMode::Horizontal => (view.offset.x, size.x - view.viewport.x),
        _ => (view.offset.y, size.y - view.viewport.y),
    };
    // Paged modes have nowhere to drift; at the strip's end it is done.
    if at.mode.paged() || here >= end - 0.5 {
        view.drift = None;
        return None;
    }
    view.drift = Some(now);
    ctx.request_repaint();
    // A held press, as a selection's drag, holds it still; so does a jump.
    if view.jump.is_some() || ctx.input(|i| i.pointer.any_down()) {
        return None;
    }
    let dt = (now - last).clamp(0.0, MAX_FRAME) as f32;
    Some((AUTO_SPEED * dt).min(end - here))
}

/// The keys' scroll along the strip, in points.
fn steered(view: &mut PdfView, at: &Place, ctx: &egui::Context, len: f32, now: f64) -> Option<f32> {
    if at.mode.paged() {
        view.hold = None;
        view.nudge = None;
        view.aim = None;
        return None;
    }
    let here = if at.mode == ViewMode::Horizontal {
        view.offset.x
    } else {
        view.offset.y
    };
    // A wheel or a drag moved the strip meanwhile: the reader wins.
    if view.aim.is_some_and(|g| (g.at - here).abs() > 1.0) {
        view.aim = None;
    }
    let mut keyed = None;
    if let Some(n) = view.nudge.take() {
        // A key pressed mid-glide adds to where that glide was going.
        let to = view.aim.map_or(here, |g| g.to) + n.points(len);
        let a = motion::jump(ctx);
        view.aim = (a.duration.max > 0.0).then(|| Glide::new(here, to, now, a));
        keyed = Some(to - here);
    }
    if let Some(g) = &mut view.aim {
        let (at, done) = g.step(now);
        keyed = Some(at - here);
        if done {
            view.aim = None;
        } else {
            ctx.request_repaint();
        }
    }
    let Some(mut hold) = view.hold else {
        return keyed;
    };
    if !ctx.input(|i| keymap::held(i, hold.arrow)) {
        view.hold = None;
        return keyed;
    }
    if now - hold.since >= HOLD_DELAY {
        view.aim = None;
        let dt = (now - hold.last).clamp(0.0, MAX_FRAME) as f32;
        keyed = Some(hold.arrow.sign() * HOLD_SPEED * dt);
    }
    hold.last = now;
    view.hold = Some(hold);
    ctx.request_repaint();
    keyed
}
