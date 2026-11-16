//! Raster effect helpers: 2D transforms, RGBA shadows, directional tiling.
use std::sync::Arc;

use vello_cpu::color::AlphaColor;
use vello_cpu::color::Srgb;
use vello_cpu::kurbo::{Affine, Rect, RoundedRect, Shape};
use vello_cpu::peniko::Fill;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::anim::{Repeat, Transform2d};
use crate::block::{Backdrop, Finish, Layer, Shadow, TextShadow};
use crate::flow::interactive::TextHoverOverride;
use crate::flow::{Deco, Laid, Piece, Set};
use crate::picture::{Decoded, Picture};

use crate::raster::{BODY, MAX_SHADOWS, MAX_TILES, Painter, reach, round_rect};

type Color = AlphaColor<Srgb>;

/// Collects colour CSS `background-image` thumbnails to keep uninverted.
pub(crate) fn deco_pictures(
    decos: &[Deco],
    (left, top): (f32, f32),
    max_area: f32,
    out: &mut Vec<([f32; 4], Arc<Picture>)>,
) {
    for deco in decos {
        let [x, y, w, h] = deco.rect;
        if w <= 0.0 || h <= 0.0 || w * h >= max_area {
            continue;
        }
        let rect = [left + x, top + y, w, h];
        if let Some(b) = &deco.backdrop {
            out.push((rect, Arc::clone(&b.picture)));
        }
        for layer in deco.finish.as_ref().map_or(&[][..], |f| &f.layers) {
            if let Layer::Picture(b) = layer {
                out.push((rect, Arc::clone(&b.picture)));
            }
        }
    }
}

/// Builds a kurbo `Affine` for `t` around `t.origin` inside `rect`.
pub(crate) fn transform_affine(t: Transform2d, rect: [f32; 4], em: f32) -> Affine {
    t.affine(rect, em)
}

/// Colour for a box shadow: explicit RGBA, else monochrome `fallback`.
pub(crate) fn shadow_paint(s: &Shadow, fallback: Color) -> Option<Color> {
    if let Some([r, g, b, a]) = s.colour {
        return (a > 0).then(|| Color::from_rgba8(r, g, b, a));
    }
    let strength = s.strength.clamp(0.0, 1.0);
    (strength >= 0.01).then(|| fallback.with_alpha(strength))
}

/// Colour for a text shadow: explicit RGBA, else `base` scaled by `alpha`.
pub(crate) fn text_shadow_ink(s: TextShadow, base: Color, alpha: f32, share: f32) -> Color {
    match s.colour {
        Some([r, g, b, a]) => Color::from_rgba8(r, g, b, a).multiply_alpha(share),
        None => base.multiply_alpha(alpha * share),
    }
}

/// Background fill colour for a `Deco`, preferring RGBA over monochrome.
pub(crate) fn deco_fill(deco: &Deco, body: Color) -> Option<Color> {
    if let Some([r, g, b, a]) = deco.fill_rgba {
        return (a > 0).then(|| Color::from_rgba8(r, g, b, a));
    }
    deco.fill.map(|a| body.with_alpha(a.clamp(0.0, 1.0)))
}

/// Computes tile X and Y coordinates honoring directional `Repeat { x, y }`.
pub(crate) fn repeat_axes(
    b: &Backdrop,
    (ox, oy): (f32, f32),
    rect: [f32; 4],
    (iw, ih): (f32, f32),
) -> (Vec<f32>, Vec<f32>) {
    let rep = b.repeat_axis.unwrap_or(Repeat {
        x: b.repeat,
        y: b.repeat,
    });
    let [x, y, w, h] = rect;
    let first = |o: f32, start: f32, step: f32| o - ((o - start) / step).ceil() * step;
    let tiles = |from: f32, end: f32, step: f32| {
        std::iter::successors(Some(from), move |t| Some(t + step))
            .take_while(move |t| *t < end)
            .take(MAX_TILES)
            .collect::<Vec<f32>>()
    };
    let xs = if rep.x {
        tiles(first(ox, x, iw), x + w, iw)
    } else {
        vec![ox]
    };
    let ys = if rep.y {
        tiles(first(oy, y, ih), y + h, ih)
    } else {
        vec![oy]
    };
    (xs, ys)
}

impl Painter {
    /// Creates an offscreen painter for a single interactive item.
    pub(crate) fn new(ctx: RenderContext, resources: Resources, view: Affine, scale: f32) -> Self {
        Self {
            ctx,
            resources,
            view,
            top: f64::MIN,
            bottom: f64::MAX,
            scale,
            coords: Vec::new(),
            hover_text: None,
        }
    }

    /// Flushes and renders into `pixmap`.
    pub(crate) fn finish(self, pixmap: &mut Pixmap) {
        let Painter {
            mut ctx,
            mut resources,
            ..
        } = self;
        ctx.flush();
        ctx.render(pixmap, &mut resources);
    }

    /// Draws `piece` with optional `:hover` text colour / shadow overrides.
    pub(crate) fn piece_hovered(
        &mut self,
        set: &Set,
        piece: &Piece,
        decoded: &Decoded,
        laid: impl Fn() -> Arc<Laid>,
        hover_text: TextHoverOverride,
    ) {
        let prev = self.hover_text;
        self.hover_text = hover_text;
        self.piece(set, piece, decoded, laid);
        self.hover_text = prev;
    }

    /// The box's shadows: those cast outside, or if `inset` inside.
    pub(crate) fn shadows(&mut self, f: &Finish, deco: &Deco, radius: f32, inset: bool) {
        let em = deco.em;
        let [x, y, w, h] = deco.rect;
        let mine = f.shadows.iter().rev().filter(|s| s.inset == inset);
        let mut clipped = false;
        for s in mine.take(MAX_SHADOWS) {
            let Some(paint) = shadow_paint(s, BODY) else {
                continue;
            };
            if !clipped {
                // CSS shows outer shadows only outside the box, inner inside.
                let mut clip = round_rect(deco.rect, radius, 0.0);
                if !inset {
                    let far = reach(f, em) + 1.0;
                    let around = Rect::new(
                        f64::from(x - far),
                        f64::from(y - far),
                        f64::from(x + w + far),
                        f64::from(y + h + far),
                    );
                    clip.extend(around.path_elements(0.1));
                    self.ctx.set_fill_rule(Fill::EvenOdd);
                }
                self.ctx.set_transform(self.view);
                self.ctx.push_clip_layer(&clip);
                self.ctx.set_fill_rule(Fill::NonZero);
                clipped = true;
            }
            let spread = if inset { -s.spread } else { s.spread } * em;
            let (dx, dy) = (s.at[0] * em, s.at[1] * em);
            let (l, t) = (x - spread + dx, y - spread + dy);
            let (r, b) = (x + w + spread + dx, y + h + spread + dy);
            let rect = Rect::new(
                f64::from(l),
                f64::from(t),
                f64::from(r.max(l)),
                f64::from(b.max(t)),
            );
            let corner = (radius + spread).max(0.0);
            let sigma = s.blur.max(0.0) * em / 2.0;
            self.ctx.set_transform(self.view);
            self.ctx.set_paint(paint);
            if sigma * self.scale < 0.3 && !inset {
                let path = RoundedRect::from_rect(rect, f64::from(corner)).to_path(0.1);
                self.ctx.fill_path(&path);
            } else {
                self.ctx
                    .fill_blurred_rounded_rect(&rect, corner, sigma.max(0.01), inset);
            }
        }
        if clipped {
            self.ctx.pop_layer();
        }
    }
}
