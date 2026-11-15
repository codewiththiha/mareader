//! The weights and convolution shapes inside a VITS model's ONNX file.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use candle_core::{DType, Device, Tensor};

use mareader_nn_core::wire::{self, Value, take, text, varint};

use crate::{Error, Result};

const FLOAT: u64 = 1;
const FLOAT16: u64 = 10;
const EXTERNAL: u64 = 1;

/// How one convolution node pads, strides and groups its input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ConvSpec {
    pub(crate) pads: (usize, usize),
    pub(crate) stride: usize,
    pub(crate) dilation: usize,
    pub(crate) groups: usize,
}

impl Default for ConvSpec {
    fn default() -> Self {
        Self {
            pads: (0, 0),
            stride: 1,
            dilation: 1,
            groups: 1,
        }
    }
}

/// Weights by module path, as the PyTorch model named them.
pub(crate) struct Graph {
    pub(crate) tensors: HashMap<String, Tensor>,
    pub(crate) convs: HashMap<String, ConvSpec>,
}

struct Fields<'a> {
    buf: &'a [u8],
    pos: usize,
}

fn fields(buf: &[u8]) -> Fields<'_> {
    Fields { buf, pos: 0 }
}

fn bad(what: &str) -> Error {
    Error::Load(format!("the model file is damaged ({what})"))
}

impl<'a> Fields<'a> {
    fn field(&mut self) -> Result<(u64, Value<'a>)> {
        let key = varint(self.buf, &mut self.pos).map_err(bad)?;
        let value = match key & 7 {
            0 => Value::Int(varint(self.buf, &mut self.pos).map_err(bad)?),
            1 => {
                take(self.buf, &mut self.pos, 8).map_err(bad)?;
                Value::Int(0)
            }
            2 => {
                let len = varint(self.buf, &mut self.pos).map_err(bad)?;
                let len = usize::try_from(len).map_err(|_| bad("a length"))?;
                Value::Bytes(take(self.buf, &mut self.pos, len).map_err(bad)?)
            }
            5 => {
                take(self.buf, &mut self.pos, 4).map_err(bad)?;
                Value::Fixed32(0)
            }
            _ => return Err(bad("an unknown field kind")),
        };
        Ok((key >> 3, value))
    }
}

impl<'a> Iterator for Fields<'a> {
    type Item = Result<(u64, Value<'a>)>;

    fn next(&mut self) -> Option<Self::Item> {
        (self.pos < self.buf.len()).then(|| self.field())
    }
}

/// A repeated integer, written packed or one field at a time.
fn push_ints(value: &Value<'_>, out: &mut Vec<u64>) -> Result<()> {
    wire::push_ints(value, out).map_err(bad)
}

struct Initializer<'a> {
    dims: Vec<usize>,
    dtype: u64,
    data: &'a [u8],
    external: bool,
}

fn initializer(buf: &[u8]) -> Result<(String, Initializer<'_>)> {
    let mut name = String::new();
    let mut dims = Vec::new();
    let mut init = Initializer {
        dims: Vec::new(),
        dtype: 0,
        data: &[],
        external: false,
    };
    for field in fields(buf) {
        match field? {
            (1, v) => push_ints(&v, &mut dims)?,
            (2, Value::Int(t)) => init.dtype = t,
            (4 | 9, Value::Bytes(b)) => init.data = b,
            (8, v) => name = text(&v).map_err(bad)?,
            (14, Value::Int(loc)) => init.external = loc == EXTERNAL,
            _ => {}
        }
    }
    init.dims = dims
        .into_iter()
        .map(|d| usize::try_from(d).map_err(|_| bad("a dimension")))
        .collect::<Result<_>>()?;
    Ok((name, init))
}

struct Node {
    name: String,
    op: String,
    inputs: Vec<String>,
    ints: HashMap<String, Vec<u64>>,
}

fn attribute(buf: &[u8]) -> Result<(String, Vec<u64>)> {
    let mut name = String::new();
    let mut ints = Vec::new();
    for field in fields(buf) {
        match field? {
            (1, v) => name = text(&v).map_err(bad)?,
            (3 | 8, v) => push_ints(&v, &mut ints)?,
            _ => {}
        }
    }
    Ok((name, ints))
}

fn node(buf: &[u8]) -> Result<Node> {
    let mut node = Node {
        name: String::new(),
        op: String::new(),
        inputs: Vec::new(),
        ints: HashMap::new(),
    };
    for field in fields(buf) {
        match field? {
            (1, v) => node.inputs.push(text(&v).map_err(bad)?),
            (3, v) => node.name = text(&v).map_err(bad)?,
            (4, v) => node.op = text(&v).map_err(bad)?,
            (5, Value::Bytes(b)) => {
                let (name, ints) = attribute(b)?;
                node.ints.insert(name, ints);
            }
            _ => {}
        }
    }
    Ok(node)
}

/// `enc_p.proj.weight`, as against `onnx::Conv_8959` or `/dp/Mul_output_0`.
fn is_module_path(name: &str) -> bool {
    let mut parts = name.split('.');
    let head_ok = parts
        .next()
        .is_some_and(|h| !h.is_empty() && h.chars().all(|c| c.is_ascii_lowercase() || c == '_'));
    let mut rest = 0;
    let rest_ok = parts.all(|p| {
        rest += 1;
        !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    });
    head_ok && rest_ok && rest > 0
}

/// `/dec/ups.0/ConvTranspose` is the node of module `dec.ups.0`.
fn module_of(node: &str) -> Option<String> {
    let (path, _op) = node.strip_prefix('/')?.rsplit_once('/')?;
    (!path.is_empty()).then(|| path.replace('/', "."))
}

fn first(ints: Option<&Vec<u64>>, default: usize) -> usize {
    ints.and_then(|v| v.first())
        .and_then(|&v| usize::try_from(v).ok())
        .unwrap_or(default)
}

fn conv_spec(node: &Node) -> ConvSpec {
    let pads = node.ints.get("pads");
    let pad = |i: usize| {
        pads.and_then(|p| p.get(i))
            .and_then(|&v| usize::try_from(v).ok())
            .unwrap_or(0)
    };
    ConvSpec {
        pads: (pad(0), pad(1)),
        stride: first(node.ints.get("strides"), 1).max(1),
        dilation: first(node.ints.get("dilations"), 1).max(1),
        groups: first(node.ints.get("group"), 1).max(1),
    }
}

fn to_tensor(name: &str, init: &Initializer<'_>, device: &Device) -> Result<Tensor> {
    if init.external {
        return Err(Error::Load(format!(
            "{name} is stored outside the model file"
        )));
    }
    let dtype = match init.dtype {
        FLOAT => DType::F32,
        FLOAT16 => DType::F16,
        other => return Err(Error::Load(format!("{name} has data type {other}"))),
    };
    let count = init
        .dims
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| bad("a tensor size"))?;
    let bytes = count
        .checked_mul(dtype.size_in_bytes())
        .ok_or_else(|| bad("a tensor size"))?;
    if bytes != init.data.len() {
        return Err(bad(&format!("{name} holds the wrong amount of data")));
    }
    let t = Tensor::from_raw_buffer(init.data, dtype, &init.dims, device)?;
    Ok(t.to_dtype(DType::F32)?)
}

fn is_float(init: &Initializer<'_>) -> bool {
    matches!(init.dtype, FLOAT | FLOAT16)
}

/// Every float weight, the anonymous ones named by the module using them.
pub(crate) fn read(bytes: &[u8]) -> Result<Graph> {
    let mut graph_buf = None;
    for field in fields(bytes) {
        if let (7, Value::Bytes(b)) = field? {
            graph_buf = Some(b);
        }
    }
    let graph_buf = graph_buf.ok_or_else(|| Error::Load("no model graph in the file".into()))?;
    let mut inits = HashMap::new();
    let mut nodes = Vec::new();
    for field in fields(graph_buf) {
        match field? {
            (1, Value::Bytes(b)) => nodes.push(node(b)?),
            (5, Value::Bytes(b)) => {
                let (name, init) = initializer(b)?;
                inits.insert(name, init);
            }
            _ => {}
        }
    }
    let device = Device::Cpu;
    let mut tensors = HashMap::new();
    for (name, init) in &inits {
        if is_module_path(name) && is_float(init) {
            tensors.insert(name.clone(), to_tensor(name, init, &device)?);
        }
    }
    let mut convs = HashMap::new();
    for node in &nodes {
        let Some(module) = module_of(&node.name) else {
            continue;
        };
        let is_conv = matches!(node.op.as_str(), "Conv" | "ConvTranspose");
        if is_conv {
            convs.insert(module.clone(), conv_spec(node));
        }
        for (slot, input) in node.inputs.iter().enumerate() {
            let Some(init) = inits.get(input) else {
                continue;
            };
            if is_module_path(input) || !is_float(init) {
                continue;
            }
            let named = match (node.op.as_str(), slot) {
                ("Conv" | "ConvTranspose", 1) => format!("{module}.weight"),
                ("Conv" | "ConvTranspose", 2) => format!("{module}.bias"),
                ("Gather", 0) => format!("{module}.weight"),
                // Old exports keep the flow's -logs; new ones its exp.
                ("Exp", _) => {
                    let t = to_tensor(input, init, &device)?.exp()?;
                    tensors.insert(format!("{module}.scale"), t);
                    continue;
                }
                ("Mul", _) if input.ends_with("/Exp_output_0") => format!("{module}.scale"),
                _ => continue,
            };
            if let Entry::Vacant(slot) = tensors.entry(named) {
                slot.insert(to_tensor(input, init, &device)?);
            }
        }
    }
    Ok(Graph { tensors, convs })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    fn key(field: u64, kind: u64, out: &mut Vec<u8>) {
        put_varint((field << 3) | kind, out);
    }

    fn put_varint(mut v: u64, out: &mut Vec<u8>) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    pub(crate) fn bytes_field(field: u64, body: &[u8], out: &mut Vec<u8>) {
        key(field, 2, out);
        put_varint(body.len() as u64, out);
        out.extend_from_slice(body);
    }

    fn int_field(field: u64, v: u64, out: &mut Vec<u8>) {
        key(field, 0, out);
        put_varint(v, out);
    }

    pub(crate) fn tensor(name: &str, dims: &[u64], values: &[f32]) -> Vec<u8> {
        let mut t = Vec::new();
        for &d in dims {
            int_field(1, d, &mut t);
        }
        int_field(2, FLOAT, &mut t);
        bytes_field(8, name.as_bytes(), &mut t);
        let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
        bytes_field(9, &raw, &mut t);
        t
    }

    pub(crate) fn conv_node(name: &str, inputs: &[&str], dilation: u64, pads: [u64; 2]) -> Vec<u8> {
        let mut n = Vec::new();
        for i in inputs {
            bytes_field(1, i.as_bytes(), &mut n);
        }
        bytes_field(3, name.as_bytes(), &mut n);
        bytes_field(4, b"Conv", &mut n);
        let mut a = Vec::new();
        bytes_field(1, b"dilations", &mut a);
        int_field(3, dilation, &mut a);
        bytes_field(5, &a, &mut n);
        let mut a = Vec::new();
        bytes_field(1, b"pads", &mut a);
        let mut packed = Vec::new();
        put_varint(pads[0], &mut packed);
        put_varint(pads[1], &mut packed);
        bytes_field(8, &packed, &mut a);
        bytes_field(5, &a, &mut n);
        n
    }

    pub(crate) fn model(parts: &[(u64, Vec<u8>)]) -> Vec<u8> {
        let mut g = Vec::new();
        for (field, body) in parts {
            bytes_field(*field, body, &mut g);
        }
        let mut m = Vec::new();
        int_field(1, 8, &mut m);
        bytes_field(7, &g, &mut m);
        m
    }

    #[test]
    fn names_anonymous_weights_by_their_conv_and_keeps_its_shape() {
        let bytes = model(&[
            (5, tensor("onnx::Conv_7", &[2, 1, 3], &[1.0; 6])),
            (
                5,
                tensor("flow.flows.0.enc.in_layers.0.bias", &[2], &[0.5, 0.25]),
            ),
            (5, tensor("/dp/Constant_output_0", &[1], &[3.0])),
            (
                1,
                conv_node(
                    "/flow/flows.0/enc/in_layers.0/Conv",
                    &["x", "onnx::Conv_7", "flow.flows.0.enc.in_layers.0.bias"],
                    3,
                    [3, 3],
                ),
            ),
        ]);
        let graph = read(&bytes).unwrap();
        let w = &graph.tensors["flow.flows.0.enc.in_layers.0.weight"];
        assert_eq!(w.dims(), &[2, 1, 3]);
        assert!(
            graph
                .tensors
                .contains_key("flow.flows.0.enc.in_layers.0.bias")
        );
        assert!(!graph.tensors.keys().any(|k| k.starts_with('/')));
        let spec = graph.convs["flow.flows.0.enc.in_layers.0"];
        assert_eq!((spec.dilation, spec.pads, spec.groups), (3, (3, 3), 1));
    }

    #[test]
    fn damaged_files_fail_cleanly() {
        let bytes = model(&[(5, tensor("enc_p.proj.weight", &[4, 4], &[1.0; 16]))]);
        for cut in [3, bytes.len() / 2, bytes.len() - 1] {
            assert!(read(bytes.get(..cut).unwrap()).is_err(), "cut at {cut}");
        }
        let wrong = model(&[(5, tensor("enc_p.proj.weight", &[4, 5], &[1.0; 16]))]);
        assert!(read(&wrong).is_err());
        assert!(read(&[0xff; 12]).is_err());
    }

    #[test]
    fn module_paths_are_told_from_export_names() {
        assert!(is_module_path("enc_p.encoder.attn_layers.0.emb_rel_k"));
        assert!(is_module_path("emb_g.weight"));
        for name in ["sid", "onnx::Conv_1", "/dp/flows.0/Exp_output_0", "a."] {
            assert!(!is_module_path(name), "{name}");
        }
        assert_eq!(
            module_of("/dec/ups.0/ConvTranspose").as_deref(),
            Some("dec.ups.0")
        );
        assert_eq!(module_of("Conv_12"), None);
    }
}
