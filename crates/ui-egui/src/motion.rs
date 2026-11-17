//! What may animate. Off skips the in-between frames, never the end.

// Settings and the system's wish land here; views ask, not settings.

use egui::Context;
use egui::style::ScrollAnimation;
use mareader_settings::MotionSettings;

const ID: &str = "mareader-motion";

/// The frame's answer, kept in the context's data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Allowed {
    all: bool,
    icons: bool,
    zoom: bool,
    jumps: bool,
    follow: bool,
    /// The system asks for less motion: everything stops, as off does.
    reduced: bool,
}

impl Default for Allowed {
    fn default() -> Self {
        Self {
            all: true,
            icons: true,
            zoom: true,
            jumps: true,
            follow: true,
            reduced: false,
        }
    }
}

fn allowed(ctx: &Context) -> Allowed {
    ctx.data(|d| d.get_temp::<Allowed>(egui::Id::new(ID)))
        .unwrap_or_default()
}

/// Applies the Animations settings and the system's reduced-motion wish.
pub fn set(ctx: &Context, m: &MotionSettings, reduced: bool) {
    let a = Allowed {
        all: m.enabled && !reduced,
        icons: m.icon_animations() && !reduced,
        zoom: m.zoom() && !reduced,
        jumps: m.scroll_jumps() && !reduced,
        follow: m.canvas_resize() && !reduced,
        reduced,
    };
    ctx.data_mut(|d| d.insert_temp(egui::Id::new(ID), a));
    ctx.global_style_mut(|s| {
        s.animation_time = if a.all { 1.0 / 12.0 } else { 0.0 };
        s.scroll_animation = if a.all {
            ScrollAnimation::default()
        } else {
            ScrollAnimation::none()
        };
    });
}

pub fn enabled(ctx: &Context) -> bool {
    allowed(ctx).all
}

/// `secs` while animations are on, zero while they are off.
pub fn secs(ctx: &Context, secs: f32) -> f32 {
    if enabled(ctx) { secs } else { 0.0 }
}

/// `secs` while icon animations are on, zero while they are off.
pub fn icon_secs(ctx: &Context, secs: f32) -> f32 {
    if allowed(ctx).icons { secs } else { 0.0 }
}

/// A zoom step eases to its target instead of landing there.
pub fn zoom(ctx: &Context) -> bool {
    allowed(ctx).zoom
}

/// Pages re-fit on every frame of a resize, rather than once after.
pub fn follow(ctx: &Context) -> bool {
    allowed(ctx).follow
}

/// How a jump to a page, or a key's scroll, travels: glided or at once.
pub fn jump(ctx: &Context) -> ScrollAnimation {
    if allowed(ctx).jumps {
        ctx.global_style().scroll_animation
    } else {
        ScrollAnimation::none()
    }
}

/// The system asks for less motion; the loading mark fades instead.
pub fn reduced(ctx: &Context) -> bool {
    allowed(ctx).reduced
}

/// How far a view has opened, eased; 1 once settled.
pub fn entrance(ctx: &Context, opened: f64, now: f64, enter: f32) -> f32 {
    let span = secs(ctx, enter);
    if span <= 0.0 {
        return 1.0;
    }
    let x = ((now - opened) as f32 / span).clamp(0.0, 1.0);
    if x < 1.0 {
        ctx.request_repaint();
    }
    1.0 - (1.0 - x).powi(3)
}

#[derive(Clone, Copy)]
struct Slide {
    from: f32,
    to: f32,
    started: f64,
}

impl Slide {
    fn settled(value: f32, now: f64) -> Self {
        Self {
            from: value,
            to: value,
            started: now,
        }
    }

    fn eval(self, now: f64, span: f32) -> (f32, bool) {
        if (self.to - self.from).abs() < 1e-4 {
            return (self.to, false);
        }
        let x = ((now - self.started) as f32 / span).clamp(0.0, 1.0);
        let ease = 1.0 - (1.0 - x).powi(3);
        (self.from + (self.to - self.from) * ease, x < 1.0)
    }
}

/// Glides toward `target` with cubic ease-out; snaps when motion is off.
pub fn slide(ctx: &Context, id: egui::Id, target: f32, dur: f32) -> f32 {
    let span = secs(ctx, dur);
    let now = ctx.input(|i| i.time);
    let current = ctx.data(|d| d.get_temp::<Slide>(id));
    if span <= 0.0 {
        if current.is_some_and(|s| s.from != target || s.to != target) {
            ctx.data_mut(|d| d.insert_temp(id, Slide::settled(target, now)));
        }
        return target;
    }
    let Some(state) = current else {
        ctx.data_mut(|d| d.insert_temp(id, Slide::settled(target, now)));
        return target;
    };
    if (state.to - target).abs() > 1e-4 {
        let (here, _) = state.eval(now, span);
        let next = Slide {
            from: here,
            to: target,
            started: now,
        };
        ctx.data_mut(|d| d.insert_temp(id, next));
        ctx.request_repaint();
        return here;
    }
    let (val, live) = state.eval(now, span);
    if live {
        ctx.request_repaint();
    } else if (state.from - state.to).abs() >= 1e-4 {
        ctx.data_mut(|d| d.insert_temp(id, Slide::settled(target, now)));
    }
    val
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(ctx: &Context, time: f64, id: egui::Id, target: f32) -> f32 {
        let mut out = 0.0;
        let input = egui::RawInput {
            time: Some(time),
            ..Default::default()
        };
        let mut full = ctx.run_ui(input, |ui| {
            out = slide(ui.ctx(), id, target, 0.2);
        });
        full.textures_delta.clear();
        out
    }

    #[test]
    fn slide_settles_on_first_frame_and_eases_cubic_out() {
        let ctx = Context::default();
        let id = egui::Id::new("slide-test");
        assert_eq!(step(&ctx, 0.0, id, 0.0), 0.0);
        // Changing target begins smoothly from the settled value.
        assert_eq!(step(&ctx, 0.1, id, 2.0), 0.0);
        let mid = step(&ctx, 0.2, id, 2.0);
        // Cubic ease-out at t=0.5 reaches 1 - 0.5^3 = 0.875 of the span.
        assert!((mid - 1.75).abs() < 1e-3, "mid={mid}");
        assert_eq!(step(&ctx, 0.35, id, 2.0), 2.0);
        // Retargeting mid-flight starts from the live value without jumping.
        let _ = step(&ctx, 0.4, id, 0.0);
        let back = step(&ctx, 0.5, id, 0.0);
        assert!((back - 0.25).abs() < 1e-3, "back={back}");
        let pivot = step(&ctx, 0.5, id, 1.0);
        assert!((pivot - back).abs() < 1e-3, "pivot={pivot} vs {back}");
    }

    #[test]
    fn slide_snaps_when_motion_is_off() {
        let ctx = Context::default();
        let m = MotionSettings {
            enabled: false,
            ..MotionSettings::default()
        };
        set(&ctx, &m, false);
        let id = egui::Id::new("slide-off");
        assert_eq!(step(&ctx, 0.0, id, 0.0), 0.0);
        assert_eq!(step(&ctx, 0.1, id, 3.0), 3.0);
    }
}
