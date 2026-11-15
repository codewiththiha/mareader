//! Typed geometry painting and transform arithmetic.

use tiny_skia::{
    FillRule as SkFillRule, LineCap as SkLineCap, LineJoin as SkLineJoin, Paint, Path, PathBuilder,
    Pixmap, PixmapPaint, Stroke as SkStroke, StrokeDash, Transform,
};

use crate::timeline::Vals;
use crate::{Command, FillRule, LineCap, LineJoin, Node, Part, Shape};

pub(super) fn render_nodes(
    nodes: &[Node],
    parts: &[Part],
    vals: &[Vals],
    dash: Option<(f32, f32, f32)>,
    width: Option<u8>,
    transform: Transform,
    pixmap: &mut Pixmap,
) -> Option<()> {
    for node in nodes {
        match node {
            Node::Shape(shape) => {
                draw_shape(shape, vals, dash, width, transform, pixmap);
            }
            Node::Group {
                part,
                width_part,
                opacity,
                children,
            } => {
                let mut child_transform = transform;
                let mut child_opacity = opacity.clamp(0.0, 1.0);
                let (mut child_dash, child_width) = (dash, (*width_part).or(width));
                if let Some(part) = part {
                    let index = *part as usize;
                    let value = vals.get(index)?;
                    let metrics = parts.get(index)?;
                    child_transform = multiply(transform, part_transform(metrics, value));
                    if (value.op - 1.0).abs() > 1.0e-4 {
                        child_opacity *= quantise(value.op.clamp(0.0, 1.0));
                    }
                    if ((value.len - 1.0).abs() > 1.0e-4 || value.off != 0.0) && metrics.len > 0.0 {
                        let visible = quantise(value.len.clamp(0.0, 1.0) * metrics.len);
                        let hidden = quantise((1.0 - value.len.clamp(0.0, 1.0)) * metrics.len);
                        let offset = quantise((1.0 - value.off.clamp(0.0, 1.0)) * metrics.len);
                        child_dash = Some((visible, hidden, offset));
                    }
                }
                if child_opacity <= 0.0 {
                    continue;
                }
                if child_opacity < 1.0 - 1.0e-6 {
                    let mut layer = Pixmap::new(pixmap.width(), pixmap.height())?;
                    render_nodes(
                        children,
                        parts,
                        vals,
                        child_dash,
                        child_width,
                        child_transform,
                        &mut layer,
                    )?;
                    let paint = PixmapPaint {
                        opacity: child_opacity,
                        ..PixmapPaint::default()
                    };
                    pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, Transform::identity(), None);
                } else {
                    render_nodes(
                        children,
                        parts,
                        vals,
                        child_dash,
                        child_width,
                        child_transform,
                        pixmap,
                    )?;
                }
            }
        }
    }
    Some(())
}

fn draw_shape(
    shape: &Shape,
    vals: &[Vals],
    inherited_dash: Option<(f32, f32, f32)>,
    inherited_width: Option<u8>,
    transform: Transform,
    pixmap: &mut Pixmap,
) {
    let width = inherited_width
        .and_then(|part| vals.get(usize::from(part)))
        .map(|value| quantise(value.width.max(0.0)));
    draw_shape_styled(shape, inherited_dash, width, transform, pixmap);
}

fn draw_shape_styled(
    shape: &Shape,
    dash: Option<(f32, f32, f32)>,
    width: Option<f32>,
    transform: Transform,
    pixmap: &mut Pixmap,
) {
    let Some(path) = path(shape.commands) else {
        return;
    };
    if let Some(fill) = shape.fill {
        let mut paint = white(fill.opacity);
        paint.anti_alias = true;
        let rule = match fill.rule {
            FillRule::Winding => SkFillRule::Winding,
            FillRule::EvenOdd => SkFillRule::EvenOdd,
        };
        pixmap.fill_path(&path, &paint, rule, transform, None);
    }
    if let Some(stroke) = shape.stroke {
        let mut sk_stroke = SkStroke {
            width: width.unwrap_or(stroke.width),
            miter_limit: stroke.miter,
            line_cap: match stroke.cap {
                LineCap::Butt => SkLineCap::Butt,
                LineCap::Round => SkLineCap::Round,
                LineCap::Square => SkLineCap::Square,
            },
            line_join: match stroke.join {
                LineJoin::Miter => SkLineJoin::Miter,
                LineJoin::Round => SkLineJoin::Round,
                LineJoin::Bevel => SkLineJoin::Bevel,
            },
            dash: None,
        };
        if sk_stroke.width <= 0.0 {
            return;
        }
        if let Some((visible, hidden, offset)) = dash {
            // SVG permits zero intervals; tiny-skia rejects them.
            if visible <= 0.0 {
                return;
            }
            if hidden > 0.0 {
                sk_stroke.dash = StrokeDash::new(vec![visible, hidden], offset);
            }
        }
        let mut paint = white(stroke.opacity);
        paint.anti_alias = true;
        pixmap.stroke_path(&path, &paint, &sk_stroke, transform, None);
    }
}

fn white(opacity: f32) -> Paint<'static> {
    let mut paint = Paint::default();
    paint.set_color_rgba8(
        255,
        255,
        255,
        (opacity.clamp(0.0, 1.0) * 255.0).round() as u8,
    );
    paint
}

fn path(commands: &[Command]) -> Option<Path> {
    let mut builder = PathBuilder::new();
    for command in commands {
        match *command {
            Command::Move(x, y) => builder.move_to(x, y),
            Command::Line(x, y) => builder.line_to(x, y),
            Command::Quad(x1, y1, x, y) => builder.quad_to(x1, y1, x, y),
            Command::Cubic(x1, y1, x2, y2, x, y) => {
                builder.cubic_to(x1, y1, x2, y2, x, y);
            }
            Command::Close => builder.close(),
        }
    }
    builder.finish()
}

fn part_transform(part: &Part, value: &Vals) -> Transform {
    let mut transform = Matrix64::translate(decimal(part.ox + value.x), decimal(part.oy + value.y));
    transform = transform.multiply(Matrix64::scale(
        decimal(value.scale * value.scale_x),
        decimal(value.scale * value.scale_y),
    ));
    if value.rot != 0.0 {
        transform = transform.multiply(Matrix64::rotate(decimal(value.rot)));
    }
    if value.skew_x != 0.0 {
        transform = transform.multiply(Matrix64::skew_x(decimal(value.skew_x)));
    }
    if value.skew_y != 0.0 {
        transform = transform.multiply(Matrix64::skew_y(decimal(value.skew_y)));
    }
    transform
        .multiply(Matrix64::translate(decimal(-part.ox), decimal(-part.oy)))
        .tiny()
}

#[derive(Clone, Copy)]
struct Matrix64 {
    sx: f64,
    kx: f64,
    ky: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}

impl Matrix64 {
    fn translate(x: f64, y: f64) -> Self {
        Self {
            sx: 1.0,
            kx: 0.0,
            ky: 0.0,
            sy: 1.0,
            tx: x,
            ty: y,
        }
    }

    fn scale(x: f64, y: f64) -> Self {
        Self {
            sx: x,
            kx: 0.0,
            ky: 0.0,
            sy: y,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn rotate(degrees: f64) -> Self {
        let radians = degrees.to_radians();
        let (sin, cos) = radians.sin_cos();
        Self {
            sx: cos,
            kx: -sin,
            ky: sin,
            sy: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn skew_x(degrees: f64) -> Self {
        Self {
            sx: 1.0,
            kx: degrees.to_radians().tan(),
            ky: 0.0,
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn skew_y(degrees: f64) -> Self {
        Self {
            sx: 1.0,
            kx: 0.0,
            ky: degrees.to_radians().tan(),
            sy: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn multiply(self, other: Self) -> Self {
        Self {
            sx: self.sx * other.sx + self.kx * other.ky,
            kx: self.sx * other.kx + self.kx * other.sy,
            ky: self.ky * other.sx + self.sy * other.ky,
            sy: self.ky * other.kx + self.sy * other.sy,
            tx: self.sx * other.tx + self.kx * other.ty + self.tx,
            ty: self.ky * other.tx + self.sy * other.ty + self.ty,
        }
    }

    fn tiny(self) -> Transform {
        Transform::from_row(
            self.sx as f32,
            self.ky as f32,
            self.kx as f32,
            self.sy as f32,
            self.tx as f32,
            self.ty as f32,
        )
    }
}

fn decimal(value: f32) -> f64 {
    f64::from((value * 1000.0).round() as i32) / 1000.0
}

fn quantise(value: f32) -> f32 {
    decimal(value) as f32
}

fn multiply(a: Transform, b: Transform) -> Transform {
    Transform::from_row(
        a.sx * b.sx + a.kx * b.ky,
        a.ky * b.sx + a.sy * b.ky,
        a.sx * b.kx + a.kx * b.sy,
        a.ky * b.kx + a.sy * b.sy,
        a.sx * b.tx + a.kx * b.ty + a.tx,
        a.ky * b.tx + a.sy * b.ty + a.ty,
    )
}
