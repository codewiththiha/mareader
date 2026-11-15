use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use roxmltree::Node as XmlNode;

const SVG_HEAD: &str = concat!(
    r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" "#,
    r#"viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" "#,
    r#"stroke-linecap="round" stroke-linejoin="round">"#,
);

#[path = "src/source_model.rs"]
mod source_model;

use source_model::{Hole, Pack, Part, Prop, Seg};

#[path = "build/shape.rs"]
mod shape;
#[allow(clippy::approx_constant, clippy::excessive_precision)]
#[path = "src/source.rs"]
mod source;
#[path = "build/xml.rs"]
mod xml;

use shape::primitive;
use xml::{attr_f32, hidden, layer_parts, marked_body};

#[derive(Clone)]
struct FlatLayer {
    parts: Vec<u8>,
    scene: String,
}

struct Generator {
    code: String,
    serial: usize,
    flat: Vec<FlatLayer>,
}

impl Generator {
    fn new() -> Self {
        Self {
            code: String::new(),
            serial: 0,
            flat: Vec::new(),
        }
    }

    fn name(&mut self, stem: &str) -> String {
        let name = format!("{stem}_{}", self.serial);
        self.serial += 1;
        name
    }

    fn pack(&mut self, pack: &Pack, index: usize) -> Result<String, String> {
        validate(pack)?;
        self.flat.clear();
        let marked = marked_body(pack)?;
        let xml = format!("{SVG_HEAD}{marked}</svg>");
        let doc = roxmltree::Document::parse(&xml)
            .map_err(|error| format!("{}: generated XML: {error}", pack.id))?;
        let root = doc.root_element();
        for child in root.children().filter(|node| node.is_element()) {
            let scene = self.nodes(&[child], &xml, pack.id)?;
            self.flat.push(FlatLayer {
                parts: layer_parts(child),
                scene,
            });
        }
        let layers = self.layers()?;
        let parts = self.parts(pack)?;
        let segs = self.segs(pack)?;
        let name = format!("PACK_{index}");
        writeln!(
            self.code,
            "static {name}: Pack = Pack {{ id: {:?}, dur: {:?}, parts: &{parts}, segs: &{segs}, layers: &{layers} }};",
            pack.id, pack.dur
        )
        .map_err(|error| error.to_string())?;
        Ok(name)
    }

    fn parts(&mut self, pack: &Pack) -> Result<String, String> {
        let name = self.name("PARTS");
        writeln!(self.code, "static {name}: [Part; {}] = [", pack.parts.len())
            .map_err(|error| error.to_string())?;
        for part in pack.parts {
            writeln!(
                self.code,
                "Part {{ ox: {:?}, oy: {:?}, len: {:?}, width: {:?} }},",
                part.ox, part.oy, part.len, part.w
            )
            .map_err(|error| error.to_string())?;
        }
        writeln!(self.code, "];").map_err(|error| error.to_string())?;
        Ok(name)
    }

    fn segs(&mut self, pack: &Pack) -> Result<String, String> {
        let name = self.name("SEGS");
        writeln!(self.code, "static {name}: [Seg; {}] = [", pack.segs.len())
            .map_err(|error| error.to_string())?;
        for seg in pack.segs {
            writeln!(
                self.code,
                "Seg {{ part: {}, prop: Prop::{:?}, t0: {:?}, dur: {:?}, keys: &{:?}, times: &{:?}, eases: &{:?} }},",
                seg.part, seg.prop, seg.t0, seg.dur, seg.keys, seg.times, seg.eases
            )
            .map_err(|error| error.to_string())?;
        }
        writeln!(self.code, "];").map_err(|error| error.to_string())?;
        Ok(name)
    }

    fn nodes(
        &mut self,
        nodes: &[XmlNode<'_, '_>],
        xml: &str,
        pack: &str,
    ) -> Result<String, String> {
        let mut values = Vec::new();
        for &node in nodes {
            match node.tag_name().name() {
                "g" => {
                    let opacity = attr_f32(node, "opacity").unwrap_or(1.0).clamp(0.0, 1.0);
                    let part = node
                        .attribute("data-mareader-part")
                        .and_then(|value| value.parse().ok());
                    let width_part = node
                        .attribute("data-mareader-width")
                        .and_then(|value| value.parse().ok());
                    let children = if opacity == 0.0 {
                        self.empty_nodes()?
                    } else {
                        let nodes = node
                            .children()
                            .filter(|child| child.is_element())
                            .collect::<Vec<_>>();
                        self.nodes(&nodes, xml, pack)?
                    };
                    values.push(format!(
                        "Node::Group {{ part: {}, width_part: {}, opacity: {:?}, children: &{children} }}",
                        option_u8(part),
                        option_u8(width_part),
                        opacity
                    ));
                }
                "path" | "rect" | "circle" | "line" => {
                    if hidden(node) {
                        continue;
                    }
                    let part = node
                        .attribute("data-mareader-part")
                        .and_then(|value| value.parse().ok());
                    let width_part = node
                        .attribute("data-mareader-width")
                        .and_then(|value| value.parse().ok());
                    let opacity = attr_f32(node, "opacity").unwrap_or(1.0).clamp(0.0, 1.0);
                    let range = node.range();
                    let element = xml
                        .get(range)
                        .ok_or_else(|| format!("{pack}: primitive range left its source"))?;
                    let mut primitive_nodes = Vec::new();
                    for shape in primitive(element, pack)? {
                        let shape_name = self.shape(shape)?;
                        primitive_nodes.push(format!("Node::Shape(&{shape_name})"));
                    }
                    if part.is_some() || width_part.is_some() || opacity < 1.0 {
                        let children = self.write_nodes(primitive_nodes)?;
                        values.push(format!(
                            "Node::Group {{ part: {}, width_part: {}, opacity: {:?}, children: &{children} }}",
                            option_u8(part),
                            option_u8(width_part),
                            opacity
                        ));
                    } else {
                        values.extend(primitive_nodes);
                    }
                }
                tag => return Err(format!("{pack}: unsupported element <{tag}>")),
            }
        }
        self.write_nodes(values)
    }

    fn layers(&mut self) -> Result<String, String> {
        let flat = self.flat.clone();
        let mut values = Vec::new();
        for layer in flat {
            let parts = self.name("LAYER_PARTS");
            writeln!(
                self.code,
                "static {parts}: [u8; {}] = {:?};",
                layer.parts.len(),
                layer.parts
            )
            .map_err(|error| error.to_string())?;
            values.push(format!(
                "Layer {{ parts: &{parts}, scene: &{} }}",
                layer.scene
            ));
        }
        let name = self.name("LAYERS");
        writeln!(self.code, "static {name}: [Layer; {}] = [", values.len())
            .map_err(|error| error.to_string())?;
        for value in values {
            writeln!(self.code, "{value},").map_err(|error| error.to_string())?;
        }
        writeln!(self.code, "];").map_err(|error| error.to_string())?;
        Ok(name)
    }

    fn write_nodes(&mut self, values: Vec<String>) -> Result<String, String> {
        let name = self.name("NODES");
        writeln!(self.code, "static {name}: [Node; {}] = [", values.len())
            .map_err(|error| error.to_string())?;
        for value in values {
            writeln!(self.code, "{value},").map_err(|error| error.to_string())?;
        }
        writeln!(self.code, "];").map_err(|error| error.to_string())?;
        Ok(name)
    }

    fn empty_nodes(&mut self) -> Result<String, String> {
        self.write_nodes(Vec::new())
    }

    fn shape(&mut self, shape: Shape) -> Result<String, String> {
        let commands = self.name("COMMANDS");
        writeln!(
            self.code,
            "static {commands}: [Command; {}] = [",
            shape.commands.len()
        )
        .map_err(|error| error.to_string())?;
        for command in shape.commands {
            writeln!(self.code, "{command},").map_err(|error| error.to_string())?;
        }
        writeln!(self.code, "];").map_err(|error| error.to_string())?;
        let name = self.name("SHAPE");
        writeln!(
            self.code,
            "static {name}: Shape = Shape {{ commands: &{commands}, fill: {}, stroke: {} }};",
            shape.fill, shape.stroke
        )
        .map_err(|error| error.to_string())?;
        Ok(name)
    }
}

struct Shape {
    commands: Vec<String>,
    fill: String,
    stroke: String,
}

fn validate(pack: &Pack) -> Result<(), String> {
    if pack.id.is_empty() || !pack.dur.is_finite() || pack.dur <= 0.0 {
        return Err(format!("{}: invalid id or duration", pack.id));
    }
    if pack.parts.is_empty() || pack.parts.len() > usize::from(u8::MAX) + 1 {
        return Err(format!("{}: invalid part count", pack.id));
    }
    for (index, part) in pack.parts.iter().enumerate() {
        if part.name.is_empty()
            || part.len < 0.0
            || part.w < 0.0
            || ![part.ox, part.oy, part.len, part.w]
                .into_iter()
                .all(f32::is_finite)
        {
            return Err(format!("{}: invalid part {index}", pack.id));
        }
    }
    if pack
        .holes
        .iter()
        .any(|hole| usize::from(hole.part) >= pack.parts.len())
    {
        return Err(format!("{}: hole names a missing part", pack.id));
    }
    for segment in pack.segs {
        let gaps = segment.keys.len().saturating_sub(1);
        if usize::from(segment.part) >= pack.parts.len()
            || !segment.t0.is_finite()
            || !segment.dur.is_finite()
            || segment.t0 < 0.0
            || segment.dur < 0.0
            || segment.t0 + segment.dur > pack.dur + 1.0e-4
            || segment.keys.is_empty()
            || (!segment.times.is_empty() && segment.times.len() != segment.keys.len())
            || segment.eases.len() != gaps
            || segment
                .eases
                .iter()
                .any(|ease| usize::from(*ease) >= source::EASINGS.len())
            || !segment.keys.iter().copied().all(f32::is_finite)
            || !segment.times.iter().copied().all(f32::is_finite)
        {
            return Err(format!("{}: invalid timeline segment", pack.id));
        }
    }
    Ok(())
}

fn option_u8(value: Option<u8>) -> String {
    value.map_or_else(|| "None".to_owned(), |value| format!("Some({value})"))
}

fn run() -> Result<(), String> {
    println!("cargo:rerun-if-changed=src/source.rs");
    let mut generator = Generator::new();
    writeln!(
        generator.code,
        "#[derive(Clone, Copy)]\npub(crate) enum Ease {{ Linear, Bezier([f32; 4]), Spring(f32) }}\nuse Ease::{{Bezier, Linear, Spring}};"
    )
    .map_err(|error| error.to_string())?;
    writeln!(
        generator.code,
        "pub(crate) static EASINGS: [Ease; {}] = {:?};",
        source::EASINGS.len(),
        source::EASINGS
    )
    .map_err(|error| error.to_string())?;
    let mut ids = Vec::new();
    let mut packs = Vec::new();
    for (index, pack) in source::ALL.iter().enumerate() {
        if ids.contains(&pack.id) {
            return Err(format!("{}: duplicate pack id", pack.id));
        }
        ids.push(pack.id);
        packs.push(generator.pack(pack, index)?);
    }
    writeln!(
        generator.code,
        "pub(crate) static PACKS: [&Pack; {}] = [{}];",
        packs.len(),
        packs
            .iter()
            .map(|name| format!("&{name}"))
            .collect::<Vec<_>>()
            .join(",")
    )
    .map_err(|error| error.to_string())?;
    let out = PathBuf::from(std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?);
    fs::write(out.join("generated.rs"), generator.code)
        .map_err(|error| format!("write generated icon geometry: {error}"))?;
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("animated icon generation failed: {error}");
        std::process::exit(1);
    }
}
