//! Raster effect helpers: 2D transforms, RGBA shadows, directional tiling.
use std::sync::Arc;

use vello_cpu::color::AlphaColor;
use vello_cpu::color::Srgb;
use vello_cpu::kurbo::{Affine, Point, Rect, RoundedRect, Shape};
use vello_cpu::peniko::Fill;
use vello_cpu::{Pixmap, RenderContext, Resources};

use crate::anim::{Repeat, Transform2d};
use crate::block::{Backdrop, Finish, Layer, Shadow, TextShadow};
use crate::flow::interactive::TextHoverOverride;
use crate::flow::{Deco, Draw, Laid, Piece, Set, span_height};
use crate::picture::{Decoded, Picture};

use crate::raster::{BODY, MAX_SHADOWS, MAX_TILES, Painter, reach, round_rect};

type Color = AlphaColor<Srgb>;

/// Collects visible block image rectangles, including CSS transforms.
pub(crate) fn piece_pictures(
    blocks: &[Set],
    pieces: &[Piece],
    (left, top): (f32, f32),
    out: &mut Vec<([f32; 4], Arc<Picture>)>,
) {
    for piece in pieces {
        let Some(set) = blocks.get(piece.block).filter(|s| !s.hidden) else {
            continue;
        };
        let Draw::Image { rect, picture, .. } = &set.draw else {
            continue;
        };
        let bx = set.x + piece.dx;
        let [x, y, w, h] = *rect;
        let mut laid = [bx + x, piece.dy + y, w, h];
        if let Some(t) = set.transform().filter(|t| !t.is_identity()) {
            let btop = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
            let block = [bx, btop, set.width, span_height(set, piece.units.clone())];
            laid = transformed_bounds(transform_affine(t, block, set.em), laid);
        } else if set.rotate != 0.0 && set.rotate.is_finite() {
            let btop = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
            let mid = Point::new(
                f64::from(bx + set.width / 2.0),
                f64::from(btop + span_height(set, piece.units.clone()) / 2.0),
            );
            let rot = Affine::rotate_about(f64::from(set.rotate).to_radians(), mid);
            laid = transformed_bounds(rot, laid);
        }
        out.push((
            [left + laid[0], top + laid[1], laid[2], laid[3]],
            Arc::clone(picture),
        ));
    }
}

/// Collects colour CSS `background-image` boxes, keeping full-page text safe.
pub(crate) fn deco_pictures(
    decos: &[Deco],
    (blocks, pieces): (&[Set], &[Piece]),
    ((left, top), max_area): ((f32, f32), f32),
    out: &mut Vec<([f32; 4], Arc<Picture>)>,
) {
    let text_rects: Vec<[f32; 4]> = pieces
        .iter()
        .filter_map(|piece| {
            let set = blocks.get(piece.block).filter(|s| !s.hidden)?;
            if !matches!(set.draw, Draw::Text { .. } | Draw::Table(_)) {
                return None;
            }
            let btop = set.units.get(piece.units.start).map_or(0.0, |u| u.0) + piece.dy;
            let h = span_height(set, piece.units.clone());
            (set.width > 0.0 && h > 0.0).then_some([set.x + piece.dx, btop, set.width, h])
        })
        .collect();
    for deco in decos {
        if deco.finish.as_deref().is_some_and(|f| f.opacity < 0.01) {
            continue;
        }
        let laid = match deco.transform.filter(|t| !t.is_identity()) {
            Some(t) => transformed_bounds(transform_affine(t, deco.rect, deco.em), deco.rect),
            None => deco.rect,
        };
        let [x, y, w, h] = laid;
        if w <= 0.0 || h <= 0.0 {
            continue;
        }
        if w * h >= max_area && text_rects.iter().any(|&t| overlaps(laid, t)) {
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

fn overlaps([ax, ay, aw, ah]: [f32; 4], [bx, by, bw, bh]: [f32; 4]) -> bool {
    ax < bx + bw && ax + aw > bx && ay < by + bh && ay + ah > by
}

fn transformed_bounds(affine: Affine, [x, y, w, h]: [f32; 4]) -> [f32; 4] {
    let corners = [(x, y), (x + w, y), (x + w, y + h), (x, y + h)];
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for (cx, cy) in corners {
        let p = affine * Point::new(f64::from(cx), f64::from(cy));
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }
    [
        min_x as f32,
        min_y as f32,
        (max_x - min_x).max(0.0) as f32,
        (max_y - min_y).max(0.0) as f32,
    ]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block::{BackdropSize, Block, Boxed, Content, Inline, Look, Rise, Role};
    use crate::flow::{Flow, PAGE_HEIGHT, PAGE_WIDTH, Typography};
    use crate::fonts::Fonts;
    use crate::picture::tests::png;

    fn designed_look() -> Arc<Look> {
        Arc::new(Look {
            designed: true,
            ..Look::default()
        })
    }

    fn colour_picture(w: u32, h: u32) -> Option<Arc<Picture>> {
        Picture::probe(png(w, h)).ok().map(Arc::new)
    }

    #[test]
    fn hidden_block_and_veiled_inline_images_leave_pictures_empty() {
        let pic = colour_picture(120, 60);
        assert!(pic.is_some(), "probe failed");
        let Some(pic) = pic else {
            return;
        };
        let mut hidden_img = Block::new(Content::Image {
            picture: Arc::clone(&pic),
            cover: false,
            width: None,
            links: Vec::new(),
        });
        hidden_img.frame.hidden = true;
        let mut para = Block::text(Role::Paragraph, "Inline hidden");
        if let Content::Text(t) = &mut para.content {
            t.inlines.push(Inline {
                at: 6,
                picture: pic,
                width: 24.0,
                height: 24.0,
                rise: Rise::Baseline,
                link: None,
                hidden: true,
            });
        }
        let flow = Flow::new(
            Arc::new(vec![hidden_img, para]),
            &Typography::default(),
            Fonts::bundled(),
        );
        assert!(flow.pictures(0).is_empty(), "{:?}", flow.pictures(0));
    }

    #[test]
    fn transformed_image_picture_rect_follows_css_transform() {
        let pic = colour_picture(120, 60);
        assert!(pic.is_some(), "probe failed");
        let Some(pic) = pic else {
            return;
        };
        let make = |shifted: bool| {
            let mut b = Block::new(Content::Image {
                picture: Arc::clone(&pic),
                cover: false,
                width: None,
                links: Vec::new(),
            });
            b.frame.look = Some(designed_look());
            if shifted {
                b.frame.extra_mut().transform = Some(Transform2d {
                    translate_em: [4.0, 2.0],
                    ..Transform2d::IDENTITY
                });
            }
            Flow::new(Arc::new(vec![b]), &Typography::default(), Fonts::bundled())
        };
        let base = make(false);
        let moved = make(true);
        let pair = base
            .pictures(0)
            .first()
            .copied()
            .zip(moved.pictures(0).first().copied());
        assert!(pair.is_some(), "missing picture rect");
        let Some((b_rect, m_rect)) = pair else {
            return;
        };
        let em = moved.blocks.first().map_or(0.0, |b| b.em);
        assert!(em > 0.0, "expected block em");
        assert!(
            ((m_rect[0] - b_rect[0]) - 4.0 * em).abs() < 1.0,
            "dx mismatch: base={b_rect:?} moved={m_rect:?}"
        );
        assert!(
            ((m_rect[1] - b_rect[1]) - 2.0 * em).abs() < 1.0,
            "dy mismatch: base={b_rect:?} moved={m_rect:?}"
        );
        let px = moved.render(0, 1.0, [0, 0, PAGE_WIDTH as u32, PAGE_HEIGHT as u32]);
        let (cx, cy) = (
            (m_rect[0] + m_rect[2] / 2.0) as usize,
            (m_rect[1] + m_rect[3] / 2.0) as usize,
        );
        let sample = px.as_ref().and_then(|p| {
            let i = (cy * p.width as usize + cx) * 4;
            p.rgba.get(i..i + 3).map(<[u8]>::to_vec)
        });
        assert!(
            sample.as_ref().is_some_and(|rgb| rgb != &[255, 255, 255]),
            "transformed rect should cover rendered pixels: {sample:?}"
        );
    }

    #[test]
    fn empty_full_page_background_image_is_kept_unless_text_overlaps() {
        let pic = colour_picture(120, 120);
        assert!(pic.is_some(), "probe failed");
        let Some(pic) = pic else {
            return;
        };
        let big_box = Arc::new(Boxed {
            height: Some(36.0),
            backdrop: Some(Arc::new(Backdrop {
                picture: pic,
                size: BackdropSize::Cover,
                at: [0.5, 0.5],
                repeat: false,
                repeat_axis: None,
            })),
            ..Boxed::default()
        });
        let mut cover = Block::new(Content::Space(0.0));
        cover.frame.look = Some(designed_look());
        cover.frame.boxes.push(Arc::clone(&big_box));
        let empty = Flow::new(
            Arc::new(vec![cover]),
            &Typography::default(),
            Fonts::bundled(),
        );
        assert!(
            !empty.pictures(0).is_empty(),
            "empty full-page background-image cover should be preserved"
        );

        let mut prose = Block::text(Role::Paragraph, "Prose over full-page wallpaper.");
        prose.frame.look = Some(designed_look());
        prose.frame.boxes.push(big_box);
        let with_text = Flow::new(
            Arc::new(vec![prose]),
            &Typography::default(),
            Fonts::bundled(),
        );
        assert!(
            with_text.pictures(0).is_empty(),
            "full-page background-image behind prose should stay themed as paper"
        );
    }
}
