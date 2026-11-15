//! XML layer metadata and authored attribute expansion.

use std::fmt::Write as _;

use roxmltree::Node as XmlNode;

use crate::Pack;

const HOLE: char = '\u{1}';
const WIDTH_HOLE: char = '\u{2}';

pub(super) fn layer_parts(node: XmlNode<'_, '_>) -> Vec<u8> {
    fn collect(node: XmlNode<'_, '_>, parts: &mut Vec<u8>) {
        for name in ["data-mareader-part", "data-mareader-width"] {
            if let Some(part) = node.attribute(name).and_then(|value| value.parse().ok())
                && !parts.contains(&part)
            {
                parts.push(part);
            }
        }
        for child in node.children().filter(|child| child.is_element()) {
            collect(child, parts);
        }
    }

    let mut parts = Vec::new();
    collect(node, &mut parts);
    parts
}

pub(super) fn marked_body(pack: &Pack) -> Result<String, String> {
    let mut out = String::with_capacity(pack.svg.len() + pack.holes.len() * 32);
    let mut holes = pack.holes.iter();
    for ch in pack.svg.chars() {
        if ch != HOLE && ch != WIDTH_HOLE {
            out.push(ch);
            continue;
        }
        let hole = holes
            .next()
            .ok_or_else(|| format!("{}: markup has more holes than metadata", pack.id))?;
        if hole.width {
            write!(out, " data-mareader-width=\"{}\"", hole.part)
                .map_err(|error| error.to_string())?;
        } else {
            write!(out, " data-mareader-part=\"{}\"", hole.part)
                .map_err(|error| error.to_string())?;
        }
    }
    if holes.next().is_some() {
        return Err(format!("{}: metadata has more holes than markup", pack.id));
    }
    Ok(out)
}

pub(super) fn hidden(node: XmlNode<'_, '_>) -> bool {
    node.ancestors().any(|ancestor| {
        ancestor
            .attribute("opacity")
            .and_then(|value| value.parse::<f32>().ok())
            .is_some_and(|opacity| opacity <= 0.0)
    })
}

pub(super) fn attr_f32(node: XmlNode<'_, '_>, name: &str) -> Option<f32> {
    node.attribute(name).and_then(|value| value.parse().ok())
}
