//! Theme changes cross-fade: the old frame fades out over the new.

// As the web's view transitions: snap the frame, change, fade the snap.

use std::sync::Arc;

use egui::{
    Color32, ColorImage, Context, Event, Id, LayerId, Order, Rect, TextureHandle, TextureOptions,
    UserData, ViewportCommand, pos2,
};

/// How long the old frame takes to fade out, in seconds.
pub const FADE: f32 = 0.2;
/// Frames to wait for the snapshot before changing without one.
const PATIENCE: u32 = 4;
/// egui rejects a texture with a longer side than this, while a snapshot of a
/// maximised window at 2x scale easily passes it — so the snap shrinks to fit.
const MAX_SIDE: usize = 2048;

/// Tags this module's screenshot requests.
struct Snapshot;

/// A change held for its snapshot, then the fade over it.
pub struct Crossfade<T> {
    state: State<T>,
}

impl<T> Default for Crossfade<T> {
    fn default() -> Self {
        Self { state: State::Idle }
    }
}

enum State<T> {
    Idle,
    /// The change waits for a snapshot of the frame before it.
    Waiting {
        after: T,
        frames: u32,
    },
    Fading {
        image: TextureHandle,
        start: f64,
    },
}

impl<T> Crossfade<T> {
    /// Holds `after` back and asks for a snapshot of the current look.
    pub fn hold(&mut self, ctx: &Context, after: T) {
        ctx.send_viewport_cmd(ViewportCommand::Screenshot(UserData::new(Snapshot)));
        self.state = State::Waiting { after, frames: 0 };
        ctx.request_repaint();
    }

    /// The held change, now due: its snapshot came, or waiting ran out.
    pub fn poll(&mut self, ctx: &Context) -> Option<T> {
        let State::Waiting { frames, .. } = &mut self.state else {
            return None;
        };
        let image = ctx.input(|i| i.events.iter().find_map(snapshot));
        if image.is_none() && *frames < PATIENCE {
            *frames += 1;
            ctx.request_repaint();
            return None;
        }
        let state = std::mem::replace(&mut self.state, State::Idle);
        let State::Waiting { after, .. } = state else {
            return None;
        };
        if let Some(image) = image {
            let image = Arc::unwrap_or_clone(image);
            let image = ctx.load_texture("crossfade", fitted(image), TextureOptions::LINEAR);
            let start = ctx.input(|i| i.time);
            self.state = State::Fading { image, start };
        }
        Some(after)
    }

    /// The held change, given up on because another lands over it.
    pub fn take(&mut self) -> Option<T> {
        match std::mem::replace(&mut self.state, State::Idle) {
            State::Waiting { after, .. } => Some(after),
            other => {
                self.state = other;
                None
            }
        }
    }

    pub fn is_fading(&self) -> bool {
        matches!(self.state, State::Fading { .. })
    }

    /// Lays the old frame over everything, fading, till it is gone.
    pub fn paint(&mut self, ctx: &Context) {
        let State::Fading { image, start } = &self.state else {
            return;
        };
        let t = ((ctx.input(|i| i.time) - start) as f32 / FADE).clamp(0.0, 1.0);
        if t >= 1.0 {
            self.state = State::Idle;
            return;
        }
        // Ease out: most of the change shows early.
        let left = (1.0 - t) * (1.0 - t);
        let layer = LayerId::new(Order::Debug, Id::new("crossfade"));
        let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
        let tint = Color32::WHITE.gamma_multiply(left);
        let painter = ctx.layer_painter(layer);
        painter.image(image.id(), ctx.viewport_rect(), uv, tint);
        ctx.request_repaint();
    }
}

fn snapshot(event: &Event) -> Option<Arc<ColorImage>> {
    match event {
        Event::Screenshot {
            user_data, image, ..
        } => {
            let data = user_data.data.as_ref()?;
            data.downcast_ref::<Snapshot>().map(|_| Arc::clone(image))
        }
        _ => None,
    }
}

/// Box-filters `image` down until its longest side fits a texture.
///
/// The snap is only ever drawn stretched over the viewport for `FADE`, so a
/// softened copy is invisible in practice while an oversized one is a crash.
fn fitted(image: ColorImage) -> ColorImage {
    let (sw, sh) = (image.width(), image.height());
    let longest = sw.max(sh);
    if longest <= MAX_SIDE {
        return image;
    }
    // Integer maths: a rounded-down float factor can shave a pixel off the
    // side that is already exactly at the limit.
    let dw = (sw * MAX_SIDE / longest).clamp(1, sw);
    let dh = (sh * MAX_SIDE / longest).clamp(1, sh);
    let src: &[Color32] = image.pixels.as_ref();
    let mut px = vec![0_u8; dw * dh * 4];
    for (dy, band) in px.chunks_mut(dw * 4).enumerate() {
        let (y0, y1) = band_rows(dy, sh, dh);
        for (dx, out) in band.chunks_mut(4).enumerate() {
            let (x0, x1) = band_rows(dx, sw, dw);
            let (mut r, mut g, mut b, mut a, mut n) = (0_u32, 0_u32, 0_u32, 0_u32, 0_u32);
            for row in src.chunks(sw).take(y1).skip(y0) {
                for c in &row[x0..x1] {
                    r += u32::from(c.r());
                    g += u32::from(c.g());
                    b += u32::from(c.b());
                    a += u32::from(c.a());
                    n += 1;
                }
            }
            let n = n.max(1);
            out.copy_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8, (a / n) as u8]);
        }
    }
    ColorImage::from_rgba_unmultiplied([dw, dh], &px)
}

/// The half-open source span one downscaled column or row covers.
fn band_rows(part: usize, from: usize, to: usize) -> (usize, usize) {
    let lo = part * from / to;
    let hi = ((part + 1) * from / to).max(lo + 1).min(from);
    (lo, hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: usize, h: usize) -> ColorImage {
        let mut raw = vec![0_u8; w * h * 4];
        for px in raw.chunks_mut(4) {
            px.copy_from_slice(&[0x80, 0x80, 0x80, 0xff]);
        }
        ColorImage::from_rgba_premultiplied([w, h], &raw)
    }

    #[test]
    fn a_small_snap_is_passed_through_untouched() {
        let kept = fitted(image(900, 600));
        assert_eq!([kept.width(), kept.height()], [900, 600]);
    }

    #[test]
    fn an_oversized_snap_shrinks_to_the_texture_limit() {
        let shrunk = fitted(image(3000, 2000));
        assert_eq!(shrunk.width(), MAX_SIDE);
        assert_eq!(shrunk.height(), MAX_SIDE * 2000 / 3000);
    }

    #[test]
    fn a_snap_wider_than_tall_keeps_its_shape() {
        let shrunk = fitted(image(2400, 1600));
        assert_eq!(shrunk.width(), MAX_SIDE);
        assert_eq!(shrunk.height(), MAX_SIDE * 1600 / 2400);
    }

    #[test]
    fn downscaling_covers_every_target_pixel() {
        let shrunk = fitted(image(4097, 101));
        assert_eq!(shrunk.pixels.len(), shrunk.width() * shrunk.height());
        assert!(shrunk.pixels.iter().all(|c| (0x70..=0x90).contains(&c.r())));
    }
}
