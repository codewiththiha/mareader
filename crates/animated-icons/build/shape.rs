//! SVG primitive conversion into generated path and paint declarations.

use resvg::usvg;

use crate::{SVG_HEAD, Shape};

pub(super) fn primitive(element: &str, pack: &str) -> Result<Vec<Shape>, String> {
    let svg = format!("{SVG_HEAD}{element}</svg>");
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default())
        .map_err(|error| format!("{pack}: primitive did not parse: {error}"))?;
    let mut shapes = Vec::new();
    collect_paths(tree.root(), &mut shapes)?;
    if shapes.is_empty() {
        return Err(format!("{pack}: visible primitive produced no path"));
    }
    Ok(shapes)
}

fn collect_paths(group: &usvg::Group, out: &mut Vec<Shape>) -> Result<(), String> {
    for node in group.children() {
        match node {
            usvg::Node::Group(group) => collect_paths(group, out)?,
            usvg::Node::Path(path) => {
                let shape = path_shape(path)?;
                if shape.fill != "None" && shape.stroke != "None" {
                    // SVG paints fill before stroke.
                    out.push(Shape {
                        commands: shape.commands.clone(),
                        fill: shape.fill,
                        stroke: "None".to_owned(),
                    });
                    out.push(Shape {
                        commands: shape.commands,
                        fill: "None".to_owned(),
                        stroke: shape.stroke,
                    });
                } else {
                    out.push(shape);
                }
            }
            usvg::Node::Image(_) | usvg::Node::Text(_) => {
                return Err("icon primitive became a non-path node".to_owned());
            }
        }
    }
    Ok(())
}

fn path_shape(path: &usvg::Path) -> Result<Shape, String> {
    use resvg::tiny_skia::PathSegment;

    let mut commands = Vec::new();
    for segment in path.data().segments() {
        let command = match segment {
            PathSegment::MoveTo(p) => format!("Command::Move({:?}, {:?})", p.x, p.y),
            PathSegment::LineTo(p) => format!("Command::Line({:?}, {:?})", p.x, p.y),
            PathSegment::QuadTo(a, p) => {
                format!("Command::Quad({:?}, {:?}, {:?}, {:?})", a.x, a.y, p.x, p.y)
            }
            PathSegment::CubicTo(a, b, p) => format!(
                "Command::Cubic({:?}, {:?}, {:?}, {:?}, {:?}, {:?})",
                a.x, a.y, b.x, b.y, p.x, p.y
            ),
            PathSegment::Close => "Command::Close".to_owned(),
        };
        commands.push(command);
    }
    let fill = path.fill().map_or_else(
        || "None".to_owned(),
        |fill| {
            let rule = match fill.rule() {
                usvg::FillRule::NonZero => "FillRule::Winding",
                usvg::FillRule::EvenOdd => "FillRule::EvenOdd",
            };
            format!(
                "Some(Fill {{ opacity: {:?}, rule: {rule} }})",
                fill.opacity().get()
            )
        },
    );
    let stroke = path.stroke().map_or_else(
        || "None".to_owned(),
        |stroke| {
            let cap = match stroke.linecap() {
                usvg::LineCap::Butt => "LineCap::Butt",
                usvg::LineCap::Round => "LineCap::Round",
                usvg::LineCap::Square => "LineCap::Square",
            };
            let join = match stroke.linejoin() {
                usvg::LineJoin::Miter | usvg::LineJoin::MiterClip => "LineJoin::Miter",
                usvg::LineJoin::Round => "LineJoin::Round",
                usvg::LineJoin::Bevel => "LineJoin::Bevel",
            };
            format!(
                "Some(Stroke {{ opacity: {:?}, width: {:?}, cap: {cap}, join: {join}, miter: {:?} }})",
                stroke.opacity().get(),
                stroke.width().get(),
                stroke.miterlimit().get()
            )
        },
    );
    Ok(Shape {
        commands,
        fill,
        stroke,
    })
}
