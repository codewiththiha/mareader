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
