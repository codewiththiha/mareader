//! Pages drawn in greys on white with vello_cpu; the page pass themes them.
use std::f64::consts::FRAC_PI_2;
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use parley::{
    Affinity, Cursor, GlyphRun, Layout, PositionedInlineBox, PositionedLayoutItem, Selection,
};
use skrifa::{FontRef, MetadataProvider};
use vello_cpu::color::{AlphaColor, Srgb};
use vello_cpu::kurbo::{Affine, BezPath, Point, Rect, RoundedRect, Shape, Stroke};
use vello_cpu::peniko::{
    ColorStop, Extend, Gradient as Ramp, ImageAlphaType, ImageQuality, ImageSampler,
};
use vello_cpu::{Glyph, Image, ImageSource, Pixmap, RenderContext, Resources};

use crate::block::{
    Backdrop, BackdropSize, Edge, Finish, Frame9, Gradient, GradientShape, Layer, Outline,
    TextShadow, Tile,
};
use crate::flow::interactive::TextHoverOverride;
use crate::flow::{
    Deco, Draw, Flow, Grid, Initial, Laid, MARKER_GAP, Para, Piece, Set, resolve, span_height,
};
use crate::fonts::{Ink, Tone};
use crate::raster_fx;

/// A fixed page's layers: borders then pieces, bottom up.
type Layers<'a> = &'a [(Range<usize>, Range<usize>)];
use crate::picture::{Decoded, Picture};

/// The largest side a raster may have, matching the PDF path.
pub const MAX_SIDE: u32 = 4096;

type Color = AlphaColor<Srgb>;

// mareader's text inks as greys: the page pass maps black to the ink.
pub(crate) const BODY: Color = Color::from_rgba8(0, 0, 0, 255);
const SOFT: Color = Color::from_rgba8(85, 85, 85, 255);
const MUTED: Color = Color::from_rgba8(107, 107, 107, 255);
const CODE_GROUND: Color = Color::from_rgba8(0, 0, 0, 14);
const PRE_GROUND: Color = Color::from_rgba8(0, 0, 0, 12);
const BORDER: Color = Color::from_rgba8(0, 0, 0, 40);
const RULE: Color = Color::from_rgba8(0, 0, 0, 64);
/// The most dashes one border side draws.
const MAX_DASHES: usize = 4096;
const BAR: Color = Color::from_rgba8(0, 0, 0, 46);
const HEAD_GROUND: Color = Color::from_rgba8(0, 0, 0, 10);
/// Stands in for an image that won't decode.
const MISSING: Color = Color::from_rgba8(0, 0, 0, 24);
const BAR_WIDTH: f64 = 3.0;
/// A ruby note's baseline above its base's, in the base's ems.
const NOTE_RISE: f32 = 0.95;
/// The lit side of a carved or raised border, as a share of its ink.
const LIGHT_SIDE: f32 = 0.45;
/// The most shadows one box casts.
pub(crate) const MAX_SHADOWS: usize = 4;
const SQRT_2: f32 = std::f32::consts::SQRT_2;
/// A full-strength text shadow's alpha.
const TEXT_SHADOW_ALPHA: f32 = 0.6;
/// mareader draws link underlines 2 px below where fonts put them.
const UNDERLINE_GAP: f32 = 2.0;
const UNDERLINE_MIN: f32 = 0.75;
/// The most tiles a repeated background draws.
pub(crate) const MAX_TILES: usize = 256;
/// Book pixels to an em.
const BOOK_EM_PX: f32 = 16.0;

/// A book's colour, or `or` where it sets none.
fn book(colour: Option<[u8; 3]>, or: Color) -> Color {
    colour.map_or(or, |[r, g, b]| Color::from_rgba8(r, g, b, 255))
}

fn tone(ink: Ink) -> Color {
    match (ink.colour, ink.tone) {
        (Some(c), _) => book(Some(c), BODY),
        (None, Tone::Body) => BODY,
        (None, Tone::Soft) => SOFT,
        (None, Tone::Muted) => MUTED,
    }
}

/// Premultiplied RGBA8 rows, `width × height`; clear where nothing is drawn.
#[derive(Clone, PartialEq, Eq)]
pub struct Pixels {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl std::fmt::Debug for Pixels {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Pixels({}×{})", self.width, self.height)
    }
}

/// A part of a page drawn at some scale: `[x, y, width, height]` pixels.
pub type Region = [u32; 4];

impl Flow {
    /// `region` of page `page` at `scale` pixels per unit; never panics.
    pub fn render(&self, page: usize, scale: f32, region: Region) -> Option<Pixels> {
        let pieces = self.pages.get(page)?;
        let [_, _, w, h] = region;
        if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE || scale.is_nan() || scale <= 0.0 {
            return None;
        }
        let origin = self.origin(page);
        // Vertical pages are laid on their side and drawn turned.
        let turn = self.turned(page).then(|| self.page_size().0);
        // Fixed pages are laid at the book's size and drawn fitted.
        let fixed = self.fixed(page);
        let layers = fixed.map_or(&[][..], |f| f.layers.as_slice());
        let scale = scale * self.fit(page);
        catch_unwind(AssertUnwindSafe(|| {
            let decos = self.decos.get(page).map_or(&[][..], Vec::as_slice);
            self.draw((pieces, decos, layers), (scale, origin, turn), region)
        }))
        .ok()?
    }

    fn draw(
        &self,
        (pieces, decos, layers): (&[Piece], &[Deco], Layers),
        (scale, (left, top), turn): (f32, (f32, f32), Option<f32>),
        region: Region,
    ) -> Option<Pixels> {
        let [x, y, w, h] = region;
        let (w16, h16) = (u16::try_from(w).ok()?, u16::try_from(h).ok()?);
        let ctx = RenderContext::new(w16, h16);
        let resources = Resources::new();
        let s = f64::from(scale);
        // A quarter turn: laid lines run down, the first at the right.
        let turned = turn.map_or(Affine::IDENTITY, |width| {
            Affine::new([0.0, 1.0, -1.0, 0.0, f64::from(width), 0.0])
        });
        // The page area in column units, to skip what's out of view.
        let (shown_top, shown_bottom) = match turn {
            Some(_) => (f64::MIN, f64::MAX),
            None => (
                f64::from(y) / s - f64::from(top),
                f64::from(y + h) / s - f64::from(top),
            ),
        };
        let mut p = Painter {
            ctx,
            resources,
            view: Affine::translate((-f64::from(x), -f64::from(y)))
                * Affine::scale(s)
                * turned
                * Affine::translate((f64::from(left), f64::from(top))),
            top: shown_top,
            bottom: shown_bottom,
            scale,
            coords: Vec::new(),
            hover_text: None,
        };
        // Borders under text, or a fixed page's layers bottom up.
        let all = [(0..decos.len(), 0..pieces.len())];
        let layers = if layers.is_empty() { &all[..] } else { layers };
        for (borders, laid) in layers {
            for deco in decos.get(borders.clone()).unwrap_or_default() {
                p.deco(deco, &self.decoded);
            }
            for piece in pieces.get(laid.clone()).unwrap_or_default() {
                if let Some(set) = self.blocks.get(piece.block) {
                    p.piece(set, piece, &self.decoded, || self.laid(piece.block));
                }
            }
        }
        let Painter {
            mut ctx,
            mut resources,
            ..
        } = p;
        let mut pixmap = Pixmap::new(w16, h16);
        ctx.flush();
        ctx.render(&mut pixmap, &mut resources);
        // Handed over, not copied: a page at Retina scale is 40 MB.
        let rgba = pixmap.take_rgba8(ImageAlphaType::AlphaPremultiplied);
        Some(Pixels {
            width: w,
            height: h,
            rgba,
        })
    }
}

/// Draws in column units: the origin is the column's top left.
pub(crate) struct Painter {
    pub(crate) ctx: RenderContext,
    pub(crate) resources: Resources,
    pub(crate) view: Affine,
    pub(crate) top: f64,
    pub(crate) bottom: f64,
    pub(crate) scale: f32,
    pub(crate) coords: Vec<i16>,
    pub(crate) hover_text: TextHoverOverride,
}

impl Painter {
    fn fill(&mut self, color: Color, x: f32, y: f32, w: f32, h: f32) {
        self.ctx.set_transform(self.view);
        self.ctx.set_paint(color);
        let (x, y) = (f64::from(x), f64::from(y));
        let rect = Rect::new(x, y, x + f64::from(w), y + f64::from(h));
        self.ctx.fill_rect(&rect);
    }

    /// A one-unit outline inside `x, y, w, h`.
    fn frame(&mut self, color: Color, x: f32, y: f32, w: f32, h: f32) {
        self.fill(color, x, y, w, 1.0);
        self.fill(color, x, y + h - 1.0, w, 1.0);
        self.fill(color, x, y + 1.0, 1.0, h - 2.0);
        self.fill(color, x + w - 1.0, y + 1.0, 1.0, h - 2.0);
    }

    fn visible(&self, top: f32, bottom: f32) -> bool {
        f64::from(bottom) >= self.top && f64::from(top) <= self.bottom
    }

    /// Draws `piece` if in view; `laid` gives its layouts only then.
    pub(crate) fn piece(
        &mut self,
        set: &Set,
        piece: &Piece,
        decoded: &Decoded,
        laid: impl Fn() -> Arc<Laid>,
    ) {
        let bx = set.x + piece.dx;
        if let Some(t) = set.transform().filter(|t| !t.is_identity()) {
            let units = piece.units.clone();
            let top = set.units.get(units.start).map_or(0.0, |u| u.0) + piece.dy;
            let rect = [bx, top, set.width, span_height(set, units)];
            let view = self.view;
            self.view = view * raster_fx::transform_affine(t, rect, set.em);
            self.unturned(set, piece, decoded, laid);
            self.view = view;
            return;
        }
        if set.rotate == 0.0 || !set.rotate.is_finite() {
            return self.unturned(set, piece, decoded, laid);
        }
        let units = piece.units.clone();
        let top = set.units.get(units.start).map_or(0.0, |u| u.0) + piece.dy;
        let middle = Point::new(
            f64::from(bx + set.width / 2.0),
            f64::from(top + span_height(set, units) / 2.0),
        );
        let view = self.view;
        self.view = view * Affine::rotate_about(f64::from(set.rotate).to_radians(), middle);
        self.unturned(set, piece, decoded, laid);
        self.view = view;
    }

    fn unturned(
        &mut self,
        set: &Set,
        piece: &Piece,
        decoded: &Decoded,
        laid: impl Fn() -> Arc<Laid>,
    ) {
        let units = piece.units.clone();
        let top = set.units.get(units.start).map_or(0.0, |u| u.0) + piece.dy;
        let height = span_height(set, units.clone());
        if !self.visible(top - 8.0, top + height + 8.0) || set.hidden {
            return;
        }
        let bx = set.x + piece.dx;
        for &bar in &set.bars {
            self.fill(BAR, bar + piece.dx, top, BAR_WIDTH as f32, height);
        }
        match &set.draw {
            Draw::Text { pad, shaded } => {
                if *shaded {
                    self.fill(PRE_GROUND, bx, top, set.width, height);
                    self.frame(BORDER, bx, top, set.width, height);
                }
                let laid = laid();
                let Laid::Text {
                    para,
                    marker,
                    initial,
                } = &*laid
                else {
                    return;
                };
                let x = bx + pad;
                if units.start == 0
                    && let Some(marker) = marker
                {
                    let right = set.rtl.then_some(bx + set.width - pad);
                    self.marker(&para.layout, marker, (x, right, piece.dy), decoded);
                }
                if units.start == 0
                    && let Some(initial) = initial
                {
                    self.initial(&para.layout, initial, x, piece.dy);
                }
                self.lines(para, units, x, piece.dy);
                self.notes(para, piece.units.clone(), x, piece.dy);
                self.inlines(para, piece.units.clone(), (x, piece.dy), decoded);
            }
            Draw::Rule => {
                let y = top + height / 2.0 - 0.5;
                self.fill(RULE, bx, y, set.width, 1.0);
            }
            Draw::Image {
                picture,
                rect,
                turned,
                ..
            } => {
                let [x, y, w, h] = *rect;
                let rect = [bx + x, piece.dy + y, w, h];
                match turned {
                    true => self.standing(picture, decoded, rect),
                    false => self.image(picture, decoded, rect[0], rect[1], w, h),
                }
            }
            Draw::Table(grid) => {
                if let Laid::Table(rows) = &*laid() {
                    self.table(set, bx, (grid, rows), (units, piece.dy), decoded);
                }
            }
            Draw::Space => {}
        }
    }

    /// A book box's borders, in the ink the page pass themes.
    pub(crate) fn deco(&mut self, deco: &Deco, decoded: &Decoded) {
        let [_, y, w, h] = deco.rect;
        let finish = deco.finish.as_deref();
        let reach = finish.map_or(0.0, |f| reach(f, deco.em)) + 8.0;
        if !self.visible(y - reach, y + h + reach) {
            return;
        }
        let opacity = finish.map_or(1.0, |f| f.opacity.clamp(0.0, 1.0));
        if opacity < 0.01 {
            return;
        }
        let saved = self.view;
        if let Some(t) = deco.transform.filter(|t| !t.is_identity()) {
            self.view = saved * raster_fx::transform_affine(t, deco.rect, deco.em);
        }
        let faded = opacity < 0.99;
        if faded {
            self.ctx.push_opacity_layer(opacity);
        }
        let radius = deco.radius.min(w / 2.0).min(h / 2.0).max(0.0);
        let shape = (radius >= 0.5).then(|| round_rect(deco.rect, radius, 0.0));
        if let Some(f) = finish {
            self.shadows(f, deco, radius, false);
        }
        self.ctx.set_transform(self.view);
        if let Some(paint) = raster_fx::deco_fill(deco, BODY) {
            self.ctx.set_paint(paint);
            match &shape {
                Some(path) => self.ctx.fill_path(path),
                None => self.ctx.fill_rect(&rect_of(deco.rect)),
            }
        }
        let layered = finish.is_some_and(|f| !f.layers.is_empty());
        if layered || deco.backdrop.is_some() {
            let clip = shape
                .clone()
                .unwrap_or_else(|| rect_of(deco.rect).to_path(0.1));
            self.ctx.push_clip_layer(&clip);
            for layer in finish.map_or(&[][..], |f| &f.layers).iter().rev() {
                match layer {
                    Layer::Picture(b) => self.backdrop(b, deco.em, deco.rect, decoded),
                    Layer::Gradient(g) => self.gradient(g, deco.rect, deco.em),
                }
            }
            if let Some(backdrop) = &deco.backdrop {
                self.backdrop(backdrop, deco.em, deco.rect, decoded);
            }
            self.ctx.set_transform(self.view);
            self.ctx.reset_paint_transform();
            self.ctx.pop_layer();
        }
        if let Some(f) = finish {
            self.shadows(f, deco, radius, true);
        }
        match finish.and_then(|f| f.frame.as_ref()) {
            Some(frame) => self.frame9(frame, deco, decoded),
            None if radius >= 0.5 => self.rounded(deco, radius),
            None => self.edges(deco),
        }
        if let Some(outline) = finish.and_then(|f| f.outline) {
            self.outline(&outline, deco, radius);
        }
        if faded {
            self.ctx.pop_layer();
        }
        self.view = saved;
    }

    /// A round-cornered box's borders.
    fn rounded(&mut self, deco: &Deco, radius: f32) {
        let first = deco.edges[0];
        let same = deco.edges.iter().all(|e| *e == first)
            && deco.colours.iter().all(|c| *c == deco.colours[0]);
        if !same {
            // Mixed sides keep square corners.
            self.edges(deco);
            return;
        }
        let least = 1.0 / self.scale.max(0.01);
        let thick = deco.border.iter().fold(0.0f32, |a, b| a.max(*b)).max(least);
        let ink = book(deco.colours[0], BODY);
        self.ring(deco.rect, radius, (first, ink), thick);
    }

    /// A line `thick` inside `rect`'s rounded edge, styled as `edge`.
    fn ring(&mut self, rect: [f32; 4], radius: f32, (edge, ink): (Edge, Color), thick: f32) {
        let line = |inset: f32, width: f32, dashes: Option<[f64; 2]>| {
            let stroke = Stroke::new(f64::from(width));
            let stroke = match dashes {
                Some(d) => stroke.with_dashes(0.0, d),
                None => stroke,
            };
            (round_rect(rect, radius, inset), stroke, 1.0)
        };
        let t = f64::from(thick);
        let half = thick / 2.0;
        let strokes: Vec<(BezPath, Stroke, f32)> = match edge {
            Edge::None | Edge::Clear => Vec::new(),
            Edge::Double if thick * self.scale >= 3.0 => {
                let third = thick / 3.0;
                vec![
                    line(third / 2.0, third, None),
                    line(thick - third / 2.0, third, None),
                ]
            }
            Edge::Groove | Edge::Ridge => {
                let (outer, inner) = if edge == Edge::Groove {
                    (1.0, LIGHT_SIDE)
                } else {
                    (LIGHT_SIDE, 1.0)
                };
                let (mut a, mut b) = (line(half / 2.0, half, None), line(half * 1.5, half, None));
                (a.2, b.2) = (outer, inner);
                vec![a, b]
            }
            Edge::Solid | Edge::Double | Edge::Inset | Edge::Outset => {
                vec![line(half, thick, None)]
            }
            Edge::Dashed => vec![line(half, thick, Some([t * 3.0, t * 2.0]))],
            Edge::Dotted => vec![line(half, thick, Some([t, t]))],
        };
        self.ctx.set_transform(self.view);
        for (path, stroke, alpha) in strokes {
            self.ctx.set_paint(ink.multiply_alpha(alpha));
            self.ctx.set_stroke(stroke);
            self.ctx.stroke_path(&path);
        }
    }

    /// A gradient across `rect`, as the book's CSS lays it.
    fn gradient(&mut self, g: &Gradient, rect: [f32; 4], em: f32) {
        let [x, y, w, h] = rect;
        if w < 0.5 || h < 0.5 || g.stops.len() < 2 {
            return;
        }
        let (cx, cy) = (f64::from(x + w / 2.0), f64::from(y + h / 2.0));
        let (length, ramp, unit) = match g.shape {
            GradientShape::Linear(degrees) => {
                let a = f64::from(degrees).to_radians();
                let (sin, cos) = a.sin_cos();
                let length = (f64::from(w) * sin).abs() + (f64::from(h) * cos).abs();
                (length.max(1.0) as f32, Some((sin, -cos)), None)
            }
            GradientShape::Radial { at, circle, near } => {
                let [ax, ay] = at.map(|a| a.clamp(0.0, 1.0));
                let (px, py) = (x + w * ax, y + h * ay);
                let (dx, dy) = match near {
                    true => ((px - x).min(x + w - px), (py - y).min(y + h - py)),
                    false => ((px - x).max(x + w - px), (py - y).max(y + h - py)),
                };
                let (rx, ry) = match (circle, near) {
                    (true, true) => (dx.min(dy), dx.min(dy)),
                    (true, false) => (dx.hypot(dy), dx.hypot(dy)),
                    (false, true) => (dx, dy),
                    (false, false) => (dx * SQRT_2, dy * SQRT_2),
                };
                (rx.max(1.0), None, Some((px, py, rx.max(1.0), ry.max(1.0))))
            }
        };
        let mut stops = gradient_stops(g, length, em);
        let mut extend = Extend::Pad;
        let (mut from, mut to) = (0.0f32, 1.0f32);
        if g.repeating
            && let (Some(first), Some(last)) = (stops.first(), stops.last())
            && last.0 - first.0 > 0.002
        {
            (from, to) = (first.0, last.0);
            for s in &mut stops {
                s.0 = (s.0 - from) / (to - from);
            }
            extend = Extend::Repeat;
        }
        let colours: Vec<ColorStop> = stops
            .iter()
            .map(|(at, a)| ColorStop::from((at.clamp(0.0, 1.0), BODY.with_alpha(*a))))
            .collect();
        let half = f64::from(length) / 2.0;
        let paint = match (ramp, unit) {
            (Some((dx, dy)), _) => {
                let along = |t: f32| {
                    let d = f64::from(t) * f64::from(length) - half;
                    (cx + dx * d, cy + dy * d)
                };
                Ramp::new_linear(along(from), along(to))
            }
            (None, Some((px, py, rx, ry))) => {
                let scale = Affine::scale_non_uniform(1.0, f64::from(ry / rx));
                let at = Affine::translate((f64::from(px), f64::from(py)));
                self.ctx.set_paint_transform(at * scale);
                Ramp::new_radial((0.0, 0.0), rx * to.max(0.01))
            }
            (None, None) => return,
        };
        self.ctx.set_transform(self.view);
        self.ctx
            .set_paint(paint.with_extend(extend).with_stops(colours.as_slice()));
        self.ctx.fill_rect(&rect_of(rect));
        self.ctx.reset_paint_transform();
    }

    /// A border picture cut in nine over the box's border.
    fn frame9(&mut self, f: &Frame9, deco: &Deco, decoded: &Decoded) {
        let em = deco.em;
        let [x, y, w, h] = deco.rect;
        let [ot, or, ob, ol] = f.outset.map(|o| o.max(0.0) * em);
        let (x, y, w, h) = (x - ol, y - ot, w + ol + or, h + ot + ob);
        let mut width = f.width.map(|v| v.max(0.0) * em);
        // Sides wider than the box shrink together, as CSS does.
        let fit = (w / (width[1] + width[3]).max(0.01))
            .min(h / (width[0] + width[2]).max(0.01))
            .min(1.0);
        for v in &mut width {
            *v *= fit;
        }
        let [dt, dr, db, dl] = width;
        let [dt, db] = [
            if deco.continued[0] { 0.0 } else { dt },
            if deco.continued[1] { 0.0 } else { db },
        ];
        let (pw, ph) = (f.picture.width as f32, f.picture.height as f32);
        if pw < 1.0 || ph < 1.0 || w < 0.5 || h < 0.5 {
            return;
        }
        let [st, sr, sb, sl] = f.slice;
        let (st, sb) = ((st * ph).min(ph), (sb * ph).min(ph));
        let (sl, sr) = ((sl * pw).min(pw), (sr * pw).min(pw));
        let (smw, smh) = ((pw - sl - sr).max(0.0), (ph - st - sb).max(0.0));
        let (mw, mh) = ((w - dl - dr).max(0.0), (h - dt - db).max(0.0));
        // Source and target: left, top, width, height.
        let cols = [
            (0.0, sl, x, dl),
            (sl, smw, x + dl, mw),
            (pw - sr, sr, x + w - dr, dr),
        ];
        let rows = [
            (0.0, st, y, dt),
            (st, smh, y + dt, mh),
            (ph - sb, sb, y + h - db, db),
        ];
        for (r, row) in rows.iter().enumerate() {
            for (c, col) in cols.iter().enumerate() {
                let middle = r == 1 && c == 1;
                if middle && !f.fill {
                    continue;
                }
                let source = [col.0, row.0, col.1, row.1];
                let target = [col.2, row.2, col.3, row.3];
                // Edges keep their breadth's scale along their length.
                let along = [
                    match (c, r) {
                        (1, 0) => Some(dt / st.max(0.01)),
                        (1, 2) => Some(db / sb.max(0.01)),
                        (1, _) => Some(dt.max(db) / st.max(sb).max(0.01)),
                        _ => None,
                    },
                    match (r, c) {
                        (1, 0) => Some(dl / sl.max(0.01)),
                        (1, 2) => Some(dr / sr.max(0.01)),
                        (1, _) => Some(dl.max(dr) / sl.max(sr).max(0.01)),
                        _ => None,
                    },
                ];
                self.slice9(f, decoded, (source, target), (along, f.tile));
            }
        }
    }

    /// One of a border picture's nine parts, from `source` into `target`.
    fn slice9(
        &mut self,
        f: &Frame9,
        decoded: &Decoded,
        ([sx, sy, sw, sh], target): ([f32; 4], [f32; 4]),
        (along, tile): ([Option<f32>; 2], [Tile; 2]),
    ) {
        let [tx, ty, tw, th] = target;
        if sw < 0.01 || sh < 0.01 || tw < 0.1 || th < 0.1 {
            return;
        }
        // Each axis: a tile's size and where tiles start.
        let axis = |scale: Option<f32>, tile: Tile, s: f32, t: f32, at: f32| {
            let size = match (tile, scale) {
                (Tile::Stretch, _) | (_, None) => return (t, vec![at]),
                (_, Some(k)) => (s * k).max(0.5),
            };
            let (size, count) = match tile {
                Tile::Round => {
                    let n = (t / size).round().max(1.0);
                    (t / n, n)
                }
                _ => (size, (t / size).ceil()),
            };
            let start = at + (t - size * count) / 2.0;
            let n = (count as usize).min(MAX_TILES);
            (size, (0..n).map(|i| start + size * i as f32).collect())
        };
        let (iw, xs) = axis(along[0], tile[0], sw, tw, tx);
        let (ih, ys) = axis(along[1], tile[1], sh, th, ty);
        let (kx, ky) = (iw / sw, ih / sh);
        let (pw, ph) = (f.picture.width as f32, f.picture.height as f32);
        let mut drawn = 0;
        for y in &ys {
            for x in &xs {
                if drawn >= MAX_TILES {
                    return;
                }
                let whole = [x - sx * kx, y - sy * ky, pw * kx, ph * ky];
                let clip = [
                    x.max(tx),
                    y.max(ty),
                    iw.min(tx + tw - x),
                    ih.min(ty + th - y),
                ];
                self.clipped(&f.picture, decoded, whole, clip);
                drawn += 1;
            }
        }
    }

    /// A line around the box's border, clear of it by its offset.
    fn outline(&mut self, o: &Outline, deco: &Deco, radius: f32) {
        let em = deco.em;
        let least = 1.0 / self.scale.max(0.01);
        let width = (o.width * em).max(least);
        let out = o.offset * em + width;
        let [x, y, w, h] = deco.rect;
        let rect = [x - out, y - out, w + 2.0 * out, h + 2.0 * out];
        let ink = book(o.colour, BODY);
        let r = if radius >= 0.5 { radius + out } else { 0.0 };
        self.ring(rect, r, (o.edge, ink), width);
    }
    /// A background picture in `rect`, sized and placed as the book asks.
    fn backdrop(&mut self, b: &Backdrop, em: f32, rect: [f32; 4], decoded: &Decoded) {
        let [x, y, w, h] = rect;
        let px = em / BOOK_EM_PX;
        let (pw, ph) = (b.picture.width as f32 * px, b.picture.height as f32 * px);
        if pw < 0.5 || ph < 0.5 || w < 0.5 || h < 0.5 {
            return;
        }
        let (iw, ih) = match b.size {
            BackdropSize::Auto => (pw, ph),
            BackdropSize::Cover => {
                let s = (w / pw).max(h / ph);
                (pw * s, ph * s)
            }
            BackdropSize::Contain => {
                let s = (w / pw).min(h / ph);
                (pw * s, ph * s)
            }
            BackdropSize::Set([sw, sh]) => {
                let sw = sw.map(|l| resolve(l, w, em));
                let sh = sh.map(|l| resolve(l, h, em));
                match (sw, sh) {
                    (Some(a), Some(b)) => (a, b),
                    (Some(a), None) => (a, a * ph / pw),
                    (None, Some(b)) => (b * pw / ph, b),
                    (None, None) => (pw, ph),
                }
            }
        };
        if !(iw >= 0.5 && ih >= 0.5 && iw.is_finite() && ih.is_finite()) {
            return;
        }
        let [ax, ay] = b.at.map(|a| a.clamp(0.0, 1.0));
        let (ox, oy) = (x + (w - iw) * ax, y + (h - ih) * ay);
        let (xs, ys) = raster_fx::repeat_axes(b, (ox, oy), rect, (iw, ih));
        let mut drawn = 0;
        for ty in &ys {
            for tx in &xs {
                if drawn >= MAX_TILES {
                    return;
                }
                self.clipped(&b.picture, decoded, [*tx, *ty, iw, ih], rect);
                drawn += 1;
            }
        }
    }

    /// `picture` drawn at `at`, only where it overlaps `clip`.
    fn clipped(&mut self, picture: &Arc<Picture>, decoded: &Decoded, at: [f32; 4], clip: [f32; 4]) {
        let [x, y, w, h] = at;
        let [cx, cy, cw, ch] = clip;
        let (l, t) = (x.max(cx), y.max(cy));
        let (r, b) = ((x + w).min(cx + cw), (y + h).min(cy + ch));
        if r <= l || b <= t || !self.visible(t, b) {
            return;
        }
        let Some(pixmap) = decoded.get(picture, w.max(h) * self.scale) else {
            return;
        };
        let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(y))));
        let scale = Affine::scale_non_uniform(f64::from(w) / pw, f64::from(h) / ph);
        self.ctx.set_paint_transform(scale);
        self.ctx.set_paint(Image {
            image: ImageSource::Pixmap(pixmap),
            sampler: ImageSampler::default().with_quality(ImageQuality::High),
        });
        let (l, t, r, b) = (l - x, t - y, r - x, b - y);
        let shown = Rect::new(f64::from(l), f64::from(t), f64::from(r), f64::from(b));
        self.ctx.fill_rect(&shown);
        self.ctx.reset_paint_transform();
    }

    /// A square box's four sides, each in its own style and colour.
    fn edges(&mut self, deco: &Deco) {
        let [x, y, w, h] = deco.rect;
        let [top, right, bottom, left] = deco.border;
        let [et, er, eb, el] = deco.edges;
        let [ct, cr, cb, cl] = deco.colours.map(|c| book(c, BODY));
        self.shaded((et, ct), [x, y, w, top], (true, true));
        self.shaded((eb, cb), [x, y + h - bottom, w, bottom], (true, false));
        self.shaded((el, cl), [x, y, left, h], (false, true));
        self.shaded((er, cr), [x + w - right, y, right, h], (false, false));
    }

    /// A side, two-toned if carved or raised; `lit` is the top or left.
    fn shaded(&mut self, (edge, ink): (Edge, Color), rect: [f32; 4], (across, lit): (bool, bool)) {
        let light = ink.multiply_alpha(LIGHT_SIDE);
        let (near, far) = if lit { (ink, light) } else { (light, ink) };
        let [x, y, w, h] = rect;
        let halves = || match across {
            true => ([x, y, w, h / 2.0], [x, y + h / 2.0, w, h / 2.0]),
            false => ([x, y, w / 2.0, h], [x + w / 2.0, y, w / 2.0, h]),
        };
        // The outer half is the half away from the box's middle.
        let (outer, inner) = match lit {
            true => halves(),
            false => {
                let (a, b) = halves();
                (b, a)
            }
        };
        match edge {
            Edge::Inset => self.edge((Edge::Solid, near), rect, across),
            Edge::Outset => self.edge((Edge::Solid, far), rect, across),
            Edge::Groove => {
                self.edge((Edge::Solid, near), outer, across);
                self.edge((Edge::Solid, far), inner, across);
            }
            Edge::Ridge => {
                self.edge((Edge::Solid, far), outer, across);
                self.edge((Edge::Solid, near), inner, across);
            }
            _ => self.edge((edge, ink), rect, across),
        }
    }

    /// One border side in `rect`, its lines running across if `across`.
    fn edge(&mut self, (edge, ink): (Edge, Color), rect: [f32; 4], across: bool) {
        let [x, y, w, h] = rect;
        // Hairlines stay visible at small scales.
        let least = 1.0 / self.scale.max(0.01);
        let (w, h) = match across {
            true => (w, h.max(least)),
            false => (w.max(least), h),
        };
        let thick = if across { h } else { w };
        match edge {
            Edge::None | Edge::Clear => {}
            Edge::Double if thick * self.scale >= 3.0 => {
                let line = thick / 3.0;
                let (far_x, far_y) = if across {
                    (x, y + h - line)
                } else {
                    (x + w - line, y)
                };
                let (lw, lh) = if across { (w, line) } else { (line, h) };
                self.fill(ink, x, y, lw, lh);
                self.fill(ink, far_x, far_y, lw, lh);
            }
            Edge::Solid
            | Edge::Double
            | Edge::Groove
            | Edge::Ridge
            | Edge::Inset
            | Edge::Outset => self.fill(ink, x, y, w, h),
            Edge::Dashed | Edge::Dotted => {
                let dash = if edge == Edge::Dashed {
                    thick * 3.0
                } else {
                    thick
                };
                let length = if across { w } else { h };
                let step = (dash * 2.0).max(least * 2.0);
                let count = ((length / step).ceil() as usize).min(MAX_DASHES);
                for i in 0..count {
                    let at = i as f32 * step;
                    let run = dash.min(length - at);
                    match across {
                        true => self.fill(ink, x + at, y, run, h),
                        false => self.fill(ink, x, y + at, w, run),
                    }
                }
            }
        }
    }

    /// The pictures in lines `lines` of `para`, its origin at `x, dy`.
    fn inlines(
        &mut self,
        para: &Para,
        lines: Range<usize>,
        (x, dy): (f32, f32),
        decoded: &Decoded,
    ) {
        if para.pictures.is_empty() {
            return;
        }
        let boxes: Vec<PositionedInlineBox> = lines
            .filter_map(|l| para.layout.get(l))
            .flat_map(|line| line.items().collect::<Vec<_>>())
            .filter_map(|item| match item {
                PositionedLayoutItem::InlineBox(b) => Some(b),
                PositionedLayoutItem::GlyphRun(_) => None,
            })
            .collect();
        for b in boxes {
            let i = usize::try_from(b.id).ok();
            let veiled = i.and_then(|i| para.veiled.get(i)).is_some_and(|v| *v);
            let picture = i.and_then(|i| para.pictures.get(i)).filter(|_| !veiled);
            let rect = [x + b.x, dy + b.y, b.width, b.height];
            match picture {
                Some(picture) if para.turned => self.standing(picture, decoded, rect),
                Some(picture) => self.image(picture, decoded, rect[0], rect[1], b.width, b.height),
                None => {}
            }
        }
    }

    /// Lines `lines` of `para`, its origin at `x, dy`.
    fn lines(&mut self, para: &Para, lines: Range<usize>, x: f32, dy: f32) {
        let layout = &para.layout;
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
        for (n, line) in lines.filter_map(|l| Some((l, layout.get(l)?))) {
            let m = line.metrics();
            // Glyphs overflow their line box a little; keep a margin.
            let slack = m.line_height;
            if !self.visible(
                m.block_min_coord + dy - slack,
                m.block_max_coord + dy + slack,
            ) {
                continue;
            }
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(run) = item else {
                    continue;
                };
                if run.style().brush.hidden {
                    continue;
                }
                if run.style().brush.code {
                    // mareader's code ground: a pad around the run.
                    let size = run.run().font_size();
                    let (top, bottom) = (run.baseline() - size, run.baseline() + size * 0.3);
                    let left = x + run.offset() - size * 0.2;
                    let w = run.advance() + size * 0.4;
                    self.fill(CODE_GROUND, left, top + dy, w, bottom - top);
                    self.ctx.set_transform(
                        self.view * Affine::translate((f64::from(x), f64::from(dy))),
                    );
                }
                if run.style().underline.is_some() {
                    self.underline(&run, x, dy);
                }
                if run.style().strikethrough.is_some() {
                    self.strike(&run, x, dy);
                }
            }
            let mut last = None;
            for item in line.items() {
                let PositionedLayoutItem::GlyphRun(glyphs) = item else {
                    continue;
                };
                if glyphs.style().brush.hidden {
                    last = None;
                    continue;
                }
                if para.turned {
                    self.turned_run(para, &glyphs, (x, dy));
                    last = Some(glyphs);
                    continue;
                }
                self.draw_glyph_run(&glyphs);
                last = Some(glyphs);
            }
            // parley breaks at a soft hyphen but leaves the hyphen to us.
            if let Some(run) = last.filter(|_| para.hyphens.binary_search(&n).is_ok()) {
                let font = run.run().font();
                let id = FontRef::from_index(font.data.as_ref(), font.index)
                    .ok()
                    .and_then(|f| f.charmap().map('-'));
                if let Some(id) = id {
                    let hyphen = Glyph {
                        id: id.to_u32(),
                        x: run.offset() + run.advance(),
                        y: run.baseline(),
                    };
                    self.glyphs(&run, std::iter::once(hyphen));
                }
            }
        }
    }

    fn draw_glyph_run(&mut self, glyphs: &GlyphRun<'_, Ink>) {
        let shadow = self
            .hover_text
            .and_then(|(_, s)| s)
            .or(glyphs.style().brush.shadow);
        if let Some(shadow) = shadow {
            self.text_shadow(glyphs, shadow);
        }
        let placed = glyphs.positioned_glyphs().map(|g| Glyph {
            id: g.id,
            x: g.x,
            y: g.y,
        });
        if let Some([r, g, b]) = self.hover_text.and_then(|(c, _)| c) {
            self.inked(glyphs, placed, Color::from_rgb8(r, g, b));
        } else {
            self.glyphs(glyphs, placed);
        }
    }

    /// A vertical run: East Asian letters stand, combined ones lie
    /// across, the rest sideways.
    fn turned_run(&mut self, para: &Para, run: &GlyphRun<'_, Ink>, (x, dy): (f32, f32)) {
        let line = self.view * Affine::translate((f64::from(x), f64::from(dy)));
        let at = run.run().text_range().start;
        let inside = |ranges: &[Range<usize>]| ranges.iter().any(|r| r.contains(&at));
        let ink = tone(run.style().brush);
        let m = run.run().font_metrics();
        // Turned about the middle of the em box, not the baseline.
        let middle = f64::from(run.baseline() - (m.ascent - m.descent) / 2.0);
        if inside(&para.combine) {
            let size = f64::from(run.run().font_size());
            let (from, wide) = (f64::from(run.offset()), f64::from(run.advance()).max(1.0));
            let centre = Point::new(from + wide / 2.0, middle);
            // Wider than an em, it is squeezed to one.
            let squeeze = Affine::translate(centre.to_vec2())
                * Affine::scale_non_uniform((size / wide).min(1.0), 1.0)
                * Affine::translate(-centre.to_vec2());
            self.ctx
                .set_transform(line * Affine::rotate_about(-FRAC_PI_2, centre) * squeeze);
            let placed = run.positioned_glyphs().map(|g| Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            });
            self.inked(run, placed, ink);
        } else if inside(&para.upright) {
            for g in run.positioned_glyphs() {
                let centre = Point::new(f64::from(g.x + g.advance / 2.0), middle);
                self.ctx
                    .set_transform(line * Affine::rotate_about(-FRAC_PI_2, centre));
                let glyph = Glyph {
                    id: g.id,
                    x: g.x,
                    y: g.y,
                };
                self.inked(run, std::iter::once(glyph), ink);
            }
        } else {
            self.ctx.set_transform(line);
            let placed = run.positioned_glyphs().map(|g| Glyph {
                id: g.id,
                x: g.x,
                y: g.y,
            });
            self.inked(run, placed, ink);
        }
        self.ctx.set_transform(line);
    }

    /// A picture laid on its side in `rect`, drawn standing in its place.
    fn standing(&mut self, picture: &Arc<Picture>, decoded: &Decoded, [x, y, w, h]: [f32; 4]) {
        let view = self.view;
        let centre = Point::new(f64::from(x + w / 2.0), f64::from(y + h / 2.0));
        self.view = view * Affine::rotate_about(-FRAC_PI_2, centre);
        let (left, top) = (x + (w - h) / 2.0, y + (h - w) / 2.0);
        self.image(picture, decoded, left, top, h, w);
        self.view = view;
    }

    /// A link's underline under `run`, a little clear of the glyphs.
    fn underline(&mut self, run: &GlyphRun<'_, Ink>, x: f32, dy: f32) {
        let m = run.run().font_metrics();
        let size = m.underline_size.max(UNDERLINE_MIN);
        let y = run.baseline() + m.underline_offset.abs() + UNDERLINE_GAP;
        let ink = tone(run.style().brush);
        self.fill(ink, x + run.offset(), y + dy, run.advance(), size);
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
    }

    /// A line through `run`, at its font's strikeout height.
    fn strike(&mut self, run: &GlyphRun<'_, Ink>, x: f32, dy: f32) {
        let m = run.run().font_metrics();
        let size = m.strikethrough_size.max(UNDERLINE_MIN);
        let y = run.baseline() - m.strikethrough_offset.abs() - size / 2.0;
        let ink = tone(run.style().brush);
        self.fill(ink, x + run.offset(), y + dy, run.advance(), size);
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(dy))));
    }

    /// `placed` in `run`'s font, size, variation and tone.
    fn glyphs(&mut self, run: &GlyphRun<'_, Ink>, placed: impl Iterator<Item = Glyph> + Clone) {
        self.inked(run, placed, tone(run.style().brush));
    }

    /// A copy of `run` behind it, ringed by copies where it blurs.
    fn text_shadow(&mut self, run: &GlyphRun<'_, Ink>, s: TextShadow) {
        let size = run.run().font_size() / 100.0;
        let [dx, dy] = s.at.map(|v| f32::from(v) * size);
        let reach = f32::from(s.blur) * size / 2.0;
        let plain = Ink {
            colour: None,
            ..run.style().brush
        };
        let alpha = f32::from(s.strength) / 255.0 * TEXT_SHADOW_ALPHA;
        let rings: &[(f32, f32)] = match reach * self.scale >= 0.5 {
            true => &[
                (0.0, 0.0),
                (-1.0, -1.0),
                (1.0, -1.0),
                (-1.0, 1.0),
                (1.0, 1.0),
            ],
            false => &[(0.0, 0.0)],
        };
        let share = if rings.len() > 1 { 0.5 } else { 1.0 };
        let ink = raster_fx::text_shadow_ink(s, tone(plain), alpha, share);
        for (rx, ry) in rings {
            let (ox, oy) = (dx + rx * reach, dy + ry * reach);
            let copy = run.positioned_glyphs().map(move |g| Glyph {
                id: g.id,
                x: g.x + ox,
                y: g.y + oy,
            });
            self.inked(run, copy, ink);
        }
    }

    /// `placed` in `run`'s font, size and variation, in `ink`.
    fn inked(
        &mut self,
        run: &GlyphRun<'_, Ink>,
        placed: impl Iterator<Item = Glyph> + Clone,
        ink: Color,
    ) {
        let font = run.run();
        self.coords.clear();
        self.coords
            .extend(font.normalized_coords().iter().map(|c| c.to_bits()));
        self.ctx.set_paint(ink);
        // A glyph the font can't draw is left blank, not fatal.
        let _ = self
            .ctx
            .glyph_run(&mut self.resources, font.font())
            .font_size(font.font_size())
            .normalized_coords(&self.coords)
            .fill_glyphs(placed);
    }

    /// The marker a gap left of `x`, or past `right`, on the first baseline.
    fn marker(
        &mut self,
        text: &Layout<Ink>,
        marker: &Para,
        (x, right, dy): (f32, Option<f32>, f32),
        decoded: &Decoded,
    ) {
        let (Some(first), Some(own)) = (text.get(0), marker.layout.get(0)) else {
            return;
        };
        let size = own
            .items()
            .find_map(|i| match i {
                PositionedLayoutItem::GlyphRun(g) => Some(g.run().font_size()),
                PositionedLayoutItem::InlineBox(_) => None,
            })
            .unwrap_or(0.0);
        let left = match right {
            Some(right) => right + MARKER_GAP * size,
            None => x - MARKER_GAP * size - marker.layout.width(),
        };
        let shift = first.metrics().baseline - own.metrics().baseline;
        self.lines(marker, 0..1, left, dy + shift);
        self.inlines(marker, 0..1, (left, dy + shift), decoded);
    }

    /// Ruby notes over their bases on lines `lines`, centred on each.
    fn notes(&mut self, para: &Para, lines: Range<usize>, x: f32, dy: f32) {
        let layout = &para.layout;
        for note in &para.ruby {
            let Some(laid) = note.para.as_deref() else {
                continue;
            };
            let caret = |i, affinity| Cursor::from_byte_index(layout, i, affinity);
            let start = caret(note.range.start, Affinity::Downstream);
            let end = caret(note.range.end, Affinity::Upstream);
            // A base broken over lines takes the note on its first part.
            let mut first = None;
            Selection::new(start, end).geometry_with(layout, |b, line| {
                if first.is_none() {
                    first = Some((b, line));
                }
            });
            let Some((b, line)) = first.filter(|(_, line)| lines.contains(line)) else {
                continue;
            };
            let (Some(base), Some(own)) = (layout.get(line), laid.layout.get(0)) else {
                continue;
            };
            let size = base.items().find_map(|item| match item {
                PositionedLayoutItem::GlyphRun(g)
                    if g.run().text_range().end > note.range.start =>
                {
                    Some(g.run().font_size())
                }
                _ => None,
            });
            let Some(size) = size else {
                continue;
            };
            let centre = x + ((b.x0 + b.x1) / 2.0) as f32;
            let left = centre - laid.layout.width() / 2.0;
            // Clear of the base's tallest letters.
            let baseline = base.metrics().baseline - size * NOTE_RISE;
            self.lines(laid, 0..1, left, dy + baseline - own.metrics().baseline);
        }
    }

    /// A drop cap at the text's left, on the baseline of its last line.
    fn initial(&mut self, text: &Layout<Ink>, initial: &Initial, x: f32, dy: f32) {
        let last = text.len().saturating_sub(1);
        let line = text.get(initial.line.saturating_sub(1).min(last));
        let (Some(line), Some(own)) = (line, initial.para.layout.get(0)) else {
            return;
        };
        let shift = line.metrics().baseline - own.metrics().baseline;
        self.lines(&initial.para, 0..1, x, dy + shift);
    }

    fn image(&mut self, picture: &Arc<Picture>, decoded: &Decoded, x: f32, y: f32, w: f32, h: f32) {
        let need = w.max(h) * self.scale;
        let Some(pixmap) = decoded.get(picture, need) else {
            return self.fill(MISSING, x, y, w, h);
        };
        let (pw, ph) = (f64::from(pixmap.width()), f64::from(pixmap.height()));
        self.ctx
            .set_transform(self.view * Affine::translate((f64::from(x), f64::from(y))));
        self.ctx.set_paint_transform(Affine::scale_non_uniform(
            f64::from(w) / pw,
            f64::from(h) / ph,
        ));
        self.ctx.set_paint(Image {
            image: ImageSource::Pixmap(pixmap),
            sampler: ImageSampler::default().with_quality(ImageQuality::High),
        });
        self.ctx
            .fill_rect(&Rect::new(0.0, 0.0, f64::from(w), f64::from(h)));
        self.ctx.reset_paint_transform();
    }

    fn table(
        &mut self,
        set: &Set,
        bx: f32,
        (grid, laid): (&Grid, &[Vec<Para>]),
        (rows, dy): (Range<usize>, f32),
        decoded: &Decoded,
    ) {
        let [pad_x, pad_y] = grid.pad;
        for r in rows {
            let (Some(&(top, bottom)), Some(cells), Some(head)) =
                (set.units.get(r), laid.get(r), grid.heads.get(r))
            else {
                continue;
            };
            let (top, h) = (top + dy, bottom - top);
            if *head {
                self.fill(HEAD_GROUND, bx, top, set.width, h);
            }
            for (c, cell) in cells.iter().enumerate() {
                let x = bx + c as f32 * grid.column;
                self.frame(book(grid.rule, BORDER), x, top, grid.column, h);
                self.lines(cell, 0..cell.layout.len(), x + pad_x, top + pad_y);
                // Formulas and small pictures in the cell's lines.
                self.inlines(
                    cell,
                    0..cell.layout.len(),
                    (x + pad_x, top + pad_y),
                    decoded,
                );
            }
        }
    }
}

/// `rect` as a kurbo rectangle.
fn rect_of([x, y, w, h]: [f32; 4]) -> Rect {
    let (l, t) = (f64::from(x), f64::from(y));
    Rect::new(l, t, l + f64::from(w.max(0.0)), t + f64::from(h.max(0.0)))
}

/// `rect` with `radius` corners, `inset` in from its edge.
pub(crate) fn round_rect(rect: [f32; 4], radius: f32, inset: f32) -> BezPath {
    let [x, y, w, h] = rect;
    let r = f64::from((radius - inset).max(0.0));
    let (l, t) = (f64::from(x + inset), f64::from(y + inset));
    let (rr, b) = (f64::from(x + w - inset), f64::from(y + h - inset));
    RoundedRect::new(l, t, rr.max(l), b.max(t), r).to_path(0.1)
}

/// How far past its border a box's shadows and outline reach.
pub(crate) fn reach(f: &Finish, em: f32) -> f32 {
    let shadow = f.shadows.iter().filter(|s| !s.inset).fold(0.0f32, |m, s| {
        let off = s.at[0].abs().max(s.at[1].abs());
        m.max(off + s.spread.max(0.0) + s.blur.max(0.0) * 1.5)
    });
    let outline = f
        .outline
        .map_or(0.0, |o| o.offset.max(0.0) + o.width.max(0.0));
    let frame = f
        .frame
        .as_ref()
        .map_or(0.0, |r| r.outset.iter().fold(0.0f32, |m, o| m.max(*o)));
    (shadow.max(outline).max(frame) * em).min(1000.0)
}

/// A gradient's stops as shares of its `length`, in order, as CSS places them.
fn gradient_stops(g: &Gradient, length: f32, em: f32) -> Vec<(f32, f32)> {
    let last = g.stops.len().saturating_sub(1);
    let mut at: Vec<Option<f32>> = g
        .stops
        .iter()
        .map(|(l, _)| l.map(|l| resolve(l, length, em) / length.max(0.01)))
        .collect();
    if let Some(first) = at.first_mut() {
        first.get_or_insert(0.0);
    }
    if let Some(end) = at.last_mut() {
        end.get_or_insert(1.0);
    }
    // No stop sits before the one ahead of it.
    let mut most = f32::MIN;
    for v in at.iter_mut().flatten() {
        most = most.max(*v);
        *v = most;
    }
    // Unplaced stops share the room between their placed neighbours.
    let mut i = 0;
    while i <= last {
        if at.get(i).is_some_and(Option::is_some) {
            i += 1;
            continue;
        }
        let before = i.saturating_sub(1);
        let after = (i..=last).find(|j| at.get(*j).is_some_and(Option::is_some));
        let after = after.unwrap_or(last);
        let a = at.get(before).copied().flatten().unwrap_or(0.0);
        let b = at.get(after).copied().flatten().unwrap_or(1.0);
        let gap = (after - before).max(1) as f32;
        for j in i..after {
            if let Some(v) = at.get_mut(j) {
                *v = Some(a + (b - a) * (j - before) as f32 / gap);
            }
        }
        i = after.max(i + 1);
    }
    at.into_iter()
        .zip(&g.stops)
        .map(|(v, (_, a))| (v.unwrap_or(0.0), a.clamp(0.0, 1.0)))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::block::{Block, Content, Emphasis, Role, Row, Table, Text};
    use crate::flow::{Flow, PAGE_HEIGHT, PAGE_WIDTH, Typography};
    use crate::fonts::Fonts;
    use crate::picture::Picture;
    use crate::picture::tests::png;

    fn sample() -> Flow {
        let h = Block::text(Role::Heading(1), "Chapter One");
        let mut p = Text::new(Role::Paragraph);
        p.push(
            "It was a bright cold day in April, and ",
            Emphasis::default(),
        );
        p.push(
            "the clocks were striking thirteen.",
            Emphasis {
                italic: true,
                ..Emphasis::default()
            },
        );
        let blocks = [h, Block::new(Content::Text(p))];
        Flow::new(
            Arc::new(blocks.to_vec()),
            &Typography::default(),
            Fonts::bundled(),
        )
    }

    fn ink(px: &super::Pixels) -> usize {
        let (pixels, _) = px.rgba.as_chunks::<4>();
        pixels
            .iter()
            .filter(|&&[r, .., a]| a > 128 && r < 128)
            .count()
    }

    fn pixel(p: &super::Pixels, x: u32, y: u32) -> Option<[u8; 4]> {
        let at = ((y * p.width + x) * 4) as usize;
        p.rgba.get(at..at + 4)?.try_into().ok()
    }

    #[test]
    fn a_page_draws_dark_text_on_clear() {
        let f = sample();
        let (w, h) = (PAGE_WIDTH as u32 / 2, PAGE_HEIGHT as u32 / 2);
        let px = f.render(0, 0.5, [0, 0, w, h]);
        let px = px.as_ref();
        assert_eq!(px.map(|p| (p.width, p.height)), Some((w, h)));
        let dark = px.map_or(0, ink);
        assert!(dark > 200, "{dark} dark pixels");
        // The corner is margin: clear, so the paper shows through.
        assert_eq!(px.and_then(|p| pixel(p, 0, 0)), Some([0, 0, 0, 0]));
    }

    #[test]
    fn a_books_colour_draws_its_glyphs_and_their_lines() {
        let mut p = Text::new(Role::Paragraph);
        let red = Emphasis {
            underline: true,
            colour: Some([204, 0, 0]),
            ..Emphasis::default()
        };
        p.push("Plain words, then ", Emphasis::default());
        p.push("a red run with a line under it.", red);
        let blocks = Arc::new(vec![Block::new(Content::Text(p))]);
        let f = Flow::new(blocks, &Typography::default(), Fonts::bundled());
        let px = f.render(0, 1.0, [0, 0, 794, 300]);
        let (pixels, _) = px
            .as_ref()
            .map_or(&[][..], |p| p.rgba.as_slice())
            .as_chunks::<4>();
        // Premultiplied: a solid red pixel is red over nothing else.
        let red = pixels
            .iter()
            .filter(|&&[r, g, b, a]| a == 255 && r > 150 && g < 40 && b < 40);
        let black = pixels
            .iter()
            .filter(|&&[r, g, b, a]| a == 255 && r < 40 && g < 40 && b < 40);
        let (red, black) = (red.count(), black.count());
        assert!(red > 100 && black > 100, "{red} red, {black} black pixels");
    }

    #[test]
    fn a_books_grid_and_marker_colours_draw() {
        let red = Some([204, 0, 0]);
        let mut item = Block::text(Role::Paragraph, "An item in a list");
        item.frame.marker = Some("•".into());
        item.frame.marker_colour = red;
        let cells = ["a", "b"].map(|s| Text {
            text: s.into(),
            ..Text::new(Role::Paragraph)
        });
        let row = Row {
            cells: cells.to_vec(),
            head: false,
        };
        let table = Table {
            rows: vec![row.clone(), row],
            rule: red,
        };
        let blocks = Arc::new(vec![item, Block::new(Content::Table(table))]);
        let f = Flow::new(blocks, &Typography::default(), Fonts::bundled());
        let px = f.render(0, 1.0, [0, 0, 794, 400]);
        let (pixels, _) = px
            .as_ref()
            .map_or(&[][..], |p| p.rgba.as_slice())
            .as_chunks::<4>();
        let red = |&&[r, g, b, a]: &&[u8; 4]| a > 200 && r > 150 && g < 40 && b < 40;
        // A bullet and a grid of thin lines, all red.
        let reds = pixels.iter().filter(red).count();
        assert!(reds > 300, "{reds} red pixels");
    }

    /// The ink a space in box `b` draws, all alphas added up.
    fn box_ink(b: crate::block::Boxed) -> u64 {
        use crate::block::Look;
        let mut block = Block::new(Content::Space(4.0));
        let look = Look {
            designed: true,
            ..Look::default()
        };
        block.frame.look = Some(Arc::new(look));
        block.frame.boxes = crate::thin_vec![Arc::new(b)];
        let f = Flow::new(
            Arc::new(vec![block]),
            &Typography::default(),
            Fonts::bundled(),
        );
        let px = f.render(0, 1.0, [0, 0, 794, 400]);
        let rgba = px.as_ref().map_or(&[][..], |p| p.rgba.as_slice());
        rgba.as_chunks::<4>()
            .0
            .iter()
            .map(|p| u64::from(p[3]))
            .sum()
    }

    #[test]
    fn shadows_gradients_frames_and_outlines_draw() {
        use crate::block::{
            Boxed, Edge, Finish, Frame9, Gradient, GradientShape, Layer, Outline, Shadow, Tile,
        };
        let base = Boxed {
            border: [0.5; 4],
            edges: [Edge::Clear; 4],
            padding: [1.0; 4],
            ..Boxed::default()
        };
        let with = |f: Finish| Boxed {
            finish: Some(Arc::new(f)),
            ..base.clone()
        };
        // A see-through border takes room and draws nothing.
        assert_eq!(box_ink(base.clone()), 0);
        let shadow = Finish {
            shadows: vec![Shadow {
                at: [0.5, 0.5],
                blur: 0.5,
                spread: 0.0,
                strength: 0.8,
                inset: false,
                colour: None,
            }],
            ..Finish::default()
        };
        let ramp = Gradient {
            shape: GradientShape::Linear(90.0),
            stops: vec![(None, 1.0), (None, 0.0)],
            repeating: false,
        };
        let outline = Outline {
            width: 0.1,
            offset: 0.2,
            edge: Edge::Dashed,
            colour: None,
        };
        let picture = Picture::probe(png(30, 30)).ok().map(Arc::new);
        let frame = picture.map(|picture| Frame9 {
            picture,
            slice: [1.0 / 3.0; 4],
            fill: false,
            width: [0.5; 4],
            outset: [0.0; 4],
            tile: [Tile::Round; 2],
        });
        let finish_of = |layers, outline, frame| Finish {
            layers,
            outline,
            frame,
            ..Finish::default()
        };
        let grad = finish_of(vec![Layer::Gradient(ramp)], None, None);
        let cases = [
            ("shadow", shadow.clone()),
            ("gradient", grad),
            ("outline", finish_of(Vec::new(), Some(outline), None)),
            ("frame", finish_of(Vec::new(), None, frame)),
        ];
        for (name, finish) in cases {
            let ink = box_ink(with(finish));
            assert!(ink > 2000, "{name}: {ink}");
        }
        let full = box_ink(with(shadow.clone()));
        let half = box_ink(with(Finish {
            opacity: 0.5,
            ..shadow
        }));
        assert!(half > 0 && half * 10 < full * 7, "{half} of {full}");
    }

    #[test]
    fn gradient_stops_spread_as_css_places_them() {
        use crate::block::{Gradient, GradientShape, Length};
        let g = Gradient {
            shape: GradientShape::Linear(180.0),
            stops: vec![
                (None, 1.0),
                (None, 0.5),
                (Some(Length::Percent(80.0)), 0.2),
                (Some(Length::Percent(40.0)), 0.0),
                (None, 0.1),
            ],
            repeating: false,
        };
        let stops = super::gradient_stops(&g, 100.0, 16.0);
        let at: Vec<f32> = stops.iter().map(|s| (s.0 * 100.0).round()).collect();
        assert_eq!(at, [0.0, 40.0, 80.0, 80.0, 100.0]);
    }

    #[test]
    fn a_tile_matches_the_same_area_of_the_whole_page() {
        let f = sample();
        let whole = f.render(0, 1.0, [0, 0, 794, 400]);
        let tile = f.render(0, 1.0, [64, 64, 256, 128]);
        assert!(whole.is_some() && tile.is_some(), "both draw");
        let (Some(whole), Some(tile)) = (whole, tile) else {
            return;
        };
        let row = |p: &super::Pixels, x: u32, y: u32, n: u32| {
            let at = ((y * p.width + x) * 4) as usize;
            p.rgba.get(at..at + (n * 4) as usize).map(<[u8]>::to_vec)
        };
        for y in (0..128).step_by(9) {
            assert_eq!(
                row(&whole, 64, 64 + y, 256),
                row(&tile, 0, y, 256),
                "row {y}"
            );
        }
    }

    #[test]
    fn out_of_range_requests_draw_nothing() {
        let f = sample();
        assert!(f.render(9, 1.0, [0, 0, 10, 10]).is_none());
        assert!(f.render(0, 1.0, [0, 0, 0, 10]).is_none());
        assert!(f.render(0, f32::NAN, [0, 0, 10, 10]).is_none());
    }

    #[test]
    fn images_draw_their_pixels_and_junk_draws_a_box() {
        let good = Picture::probe(png(200, 100)).map(Arc::new);
        let blocks: Vec<Block> = good
            .into_iter()
            .chain([Arc::new(Picture::empty())])
            .map(|picture| {
                Block::new(Content::Image {
                    picture,
                    cover: false,
                    width: None,
                    links: Vec::new(),
                })
            })
            .collect();
        let f = Flow::new(Arc::new(blocks), &Typography::default(), Fonts::bundled());
        let px = f.render(0, 1.0, [0, 0, 794, 400]);
        // Left of the picture is red, right is blue; below, a grey box.
        let red = px.as_ref().and_then(|p| pixel(p, 72 + 5, 72 + 50));
        assert!(red.is_some_and(|[r, _, b, _]| r > 200 && b < 60), "{red:?}");
        let blue = px.as_ref().and_then(|p| pixel(p, 72 + 195, 72 + 50));
        assert!(
            blue.is_some_and(|[r, _, b, _]| r < 60 && b > 200),
            "{blue:?}"
        );
        let gap = 17;
        let grey = px.as_ref().and_then(|p| pixel(p, 72, 72 + 100 + gap));
        assert!(grey.is_some_and(|[.., a]| a > 10 && a < 50), "{grey:?}");
    }
}
