//! An ONNX export of the network, read as Kokoro's named `f32` weights.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};

use candle_core::{Device, Tensor};

use mareader_nn_core::wire::{self, Value, take, text, varint};

use crate::config::{Activation, Config, Istft, Plbert};
use crate::{Error, Result};

const FLOAT: u64 = 1;
const UINT8: u64 = 2;
const INT8: u64 = 3;
const FLOAT16: u64 = 10;
const EXTERNAL: u64 = 1;

/// Exporters' prefixes on the weights they kept by name, and Kokoro's.
const ROOTS: [(&str, &str); 3] = [
    ("kmodel.", ""),
    ("encoder.", ""),
    ("decoder.decoder.", "decoder."),
];
/// Quantised weights end so; their scale and zero point sit beside.
const QUANTISED: &str = "_quantized";
/// Width of one attention head, as the phoneme encoder was trained.
const HEAD: usize = 64;
/// Dilations of each generator block, as Kokoro's.
const DILATIONS: [usize; 3] = [1, 3, 5];

/// Export paths that lost their parent, and the parent's path.
const MOVED: [(&str, &str); 8] = [
    ("text_encoder.lstms", "predictor.text_encoder.lstms"),
    ("lstm", "predictor.lstm"),
    ("shared", "predictor.shared"),
    ("duration_proj", "predictor.duration_proj"),
    ("F0", "predictor.F0"),
    ("N", "predictor.N"),
    ("F0_proj", "predictor.F0_proj"),
    ("N_proj", "predictor.N_proj"),
];

/// A stored name as Kokoro's, if the exporter kept it by name.
fn own(name: &str) -> Option<String> {
    let mut roots = ROOTS.iter();
    roots.find_map(|(from, to)| name.strip_prefix(from).map(|rest| format!("{to}{rest}")))
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
                let four = take(self.buf, &mut self.pos, 4)
                    .map_err(bad)?
                    .first_chunk::<4>()
                    .copied();
                Value::Fixed32(u32::from_le_bytes(four.unwrap_or_default()))
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

/// One stored tensor, before it is named or widened.
#[derive(Default)]
struct Stored<'a> {
    dims: Vec<usize>,
    dtype: u64,
    raw: &'a [u8],
    /// Packed `float_data`, little-endian like `raw`.
    floats: Vec<u8>,
    /// `int32_data`, where small types go when not kept raw.
    ints: Vec<u64>,
    external: bool,
}

fn stored(buf: &[u8]) -> Result<(String, Stored<'_>)> {
    let mut name = String::new();
    let mut dims = Vec::new();
    let mut s = Stored::default();
    for field in fields(buf) {
        match field? {
            (1, v) => push_ints(&v, &mut dims)?,
            (2, Value::Int(t)) => s.dtype = t,
            (4, Value::Bytes(b)) => s.floats.extend_from_slice(b),
            (4, Value::Fixed32(v)) => s.floats.extend_from_slice(&v.to_le_bytes()),
            (5, v) => push_ints(&v, &mut s.ints)?,
            (8, v) => name = text(&v).map_err(bad)?,
            (9, Value::Bytes(b)) => s.raw = b,
            (14, Value::Int(loc)) => s.external = loc == EXTERNAL,
            _ => {}
        }
    }
    s.dims = dims
        .into_iter()
        .map(|d| usize::try_from(d).map_err(|_| bad("a dimension")))
        .collect::<Result<_>>()?;
    Ok((name, s))
}

/// A half-precision number, widened.
fn half(bits: u16) -> f32 {
    let sign = u32::from(bits >> 15) << 31;
    let exp = u32::from((bits >> 10) & 0x1f);
    let frac = u32::from(bits & 0x3ff);
    match (exp, frac) {
        (0, 0) => f32::from_bits(sign),
        (0, f) => {
            let mag = f as f32 * (-24f32).exp2();
            if sign == 0 { mag } else { -mag }
        }
        (0x1f, 0) => f32::from_bits(sign | 0x7f80_0000),
        (0x1f, f) => f32::from_bits(sign | 0x7fc0_0000 | (f << 13)),
        (e, f) => f32::from_bits(sign | ((e + 112) << 23) | (f << 13)),
    }
}

/// Every value of `s` as `f32`, or none if it isn't numbers we read.
fn values(name: &str, s: &Stored<'_>) -> Result<Option<Vec<f32>>> {
    if !matches!(s.dtype, FLOAT | FLOAT16 | UINT8 | INT8) {
        return Ok(None);
    }
    if s.external {
        return Err(Error::Load(format!(
            "{name} is stored outside the model file"
        )));
    }
    let count = s
        .dims
        .iter()
        .try_fold(1usize, |n, &d| n.checked_mul(d))
        .ok_or_else(|| bad("a tensor size"))?;
    let raw = !s.raw.is_empty();
    // Small types sit in `int32_data` when not raw, one per entry.
    let low = |v: &u64| *v as u32;
    let out: Vec<f32> = match s.dtype {
        FLOAT => {
            let bytes = if raw { s.raw } else { &s.floats };
            let (words, _) = bytes.as_chunks::<4>();
            words.iter().map(|&w| f32::from_le_bytes(w)).collect()
        }
        FLOAT16 if raw => {
            let (pairs, _) = s.raw.as_chunks::<2>();
            pairs.iter().map(|&p| half(u16::from_le_bytes(p))).collect()
        }
        FLOAT16 => s.ints.iter().map(|v| half(low(v) as u16)).collect(),
        UINT8 if raw => s.raw.iter().map(|&b| f32::from(b)).collect(),
        UINT8 => s.ints.iter().map(|v| f32::from(low(v) as u8)).collect(),
        _ if raw => s.raw.iter().map(|&b| f32::from(b as i8)).collect(),
        _ => s.ints.iter().map(|v| f32::from(low(v) as i8)).collect(),
    };
    if out.len() != count {
        return Err(bad(&format!("{name} holds the wrong amount of data")));
    }
    Ok(Some(out))
}

/// `(q − zero) × scale`: whole, per first-axis slice, or per column.
fn dequantise(
    name: &str,
    q: &[f32],
    scale: &[f32],
    zero: &[f32],
    columns: bool,
) -> Result<Vec<f32>> {
    let groups = scale.len();
    let zeros = zero.len();
    if groups == 0 || (zeros != groups && zeros != 1) || !q.len().is_multiple_of(groups) {
        return Err(bad(&format!("{name} has a scale that doesn't fit")));
    }
    let per = if columns {
        1
    } else {
        (q.len() / groups).max(1)
    };
    let at = |i: usize| -> (f32, f32) {
        let g = (i / per) % groups;
        let s = scale.get(g).copied().unwrap_or_default();
        let z = zero
            .get(if zeros == 1 { 0 } else { g })
            .copied()
            .unwrap_or_default();
        (s, z)
    };
    let wide = q.iter().enumerate().map(|(i, &v)| {
        let (s, z) = at(i);
        (v - z) * s
    });
    Ok(wide.collect())
}

/// What a quantised weight's readers say about it.
#[derive(Default)]
struct Readers {
    /// The zero point a reader names, when not stored beside the weight.
    zero: HashMap<String, String>,
    /// Weights multiplied as `[in, out]`, scaled per column.
    columns: HashSet<String>,
}

fn readers(nodes: &[Node]) -> Readers {
    let mut r = Readers::default();
    let made: HashMap<&str, &Node> = nodes
        .iter()
        .flat_map(|n| n.outputs.iter().map(move |o| (o.as_str(), n)))
        .collect();
    let mut pair = |w: Option<&String>, z: Option<&String>| {
        if let (Some(w), Some(z)) = (w, z) {
            r.zero.insert(w.clone(), z.clone());
        }
    };
    for n in nodes {
        let at = |i: usize| n.inputs.get(i);
        match n.op.as_str() {
            "MatMulInteger" | "ConvInteger" => pair(at(1), at(3)),
            "DynamicQuantizeLSTM" => {
                pair(at(1), at(9));
                pair(at(2), at(11));
            }
            // A quantised table is looked up, then widened.
            "DequantizeLinear" => {
                let table = at(0).and_then(|o| made.get(o.as_str()));
                let table = table.filter(|t| t.op == "Gather");
                pair(table.and_then(|t| t.inputs.first()), at(2));
            }
            _ => {}
        }
        if n.op == "MatMulInteger"
            && let Some(w) = at(1)
        {
            r.columns.insert(w.clone());
        }
    }
    r
}

struct Node {
    name: String,
    op: String,
    inputs: Vec<String>,
    outputs: Vec<String>,
}

fn node(buf: &[u8]) -> Result<Node> {
    let mut node = Node {
        name: String::new(),
        op: String::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
    };
    for field in fields(buf) {
        match field? {
            (1, v) => node.inputs.push(text(&v).map_err(bad)?),
            (2, v) => node.outputs.push(text(&v).map_err(bad)?),
            (3, v) => node.name = text(&v).map_err(bad)?,
            (4, v) => node.op = text(&v).map_err(bad)?,
            _ => {}
        }
    }
    Ok(node)
}

/// `/text_encoder/cnn.0/cnn.0.1/Op` is the module `text_encoder.cnn.0.1`.
fn module_of(node: &str) -> Option<String> {
    let (path, _op) = node.strip_prefix('/')?.rsplit_once('/')?;
    let mut parts: Vec<&str> = Vec::new();
    for part in path.split('/') {
        // A module run again is numbered, as `query_3`.
        let part = match part.rsplit_once('_') {
            Some((base, n)) if !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) => base,
            _ => part,
        };
        let nested = parts
            .last()
            .is_some_and(|last| part.strip_prefix(*last).is_some_and(|r| r.starts_with('.')));
        if nested {
            parts.pop();
        }
        parts.push(part);
    }
    let path = parts.join(".");
    let path = match own(&format!("{path}.")) {
        Some(rooted) => rooted.trim_end_matches('.').to_owned(),
        None => path,
    };
    for (from, to) in MOVED {
        if let Some(rest) = path.strip_prefix(from)
            && (rest.is_empty() || rest.starts_with('.'))
        {
            return Some(format!("{to}{rest}"));
        }
    }
    (!path.is_empty()).then_some(path)
}

/// A weight's shape and values, as stored.
struct Weight {
    dims: Vec<usize>,
    values: Vec<f32>,
}

/// ONNX's gate order (i, o, f, c) as PyTorch's (i, f, c, o).
const GATES: [usize; 4] = [0, 2, 3, 1];

fn tensor(w: Weight) -> Result<Tensor> {
    Ok(Tensor::from_vec(w.values, w.dims, &Device::Cpu)?)
}

/// `[dirs, rows, cols]` split per direction, gates reordered by row.
fn lstm_part(t: &Tensor, dir: usize) -> Result<Tensor> {
    let t = t.get(dir)?;
    let rows = t.dim(0)? / 4;
    let gates = GATES
        .iter()
        .map(|&g| t.narrow(0, g * rows, rows))
        .collect::<candle_core::Result<Vec<_>>>()?;
    Ok(Tensor::cat(&gates, 0)?.contiguous()?)
}

/// One of an LSTM node's inputs, as PyTorch's per-direction weights.
fn lstm(
    module: &str,
    slot: usize,
    w: Weight,
    transposed: bool,
    out: &mut HashMap<String, Tensor>,
) -> Result<()> {
    let mut t = tensor(w)?;
    if transposed && slot < 3 {
        t = t.transpose(1, 2)?.contiguous()?;
    }
    for dir in 0..t.dim(0)? {
        let suffix = if dir == 0 { "" } else { "_reverse" };
        let mut put = |name: &str, t: Tensor| {
            out.entry(format!("{module}.{name}_l0{suffix}"))
                .or_insert(t);
        };
        match slot {
            1 => put("weight_ih", lstm_part(&t, dir)?),
            2 => put("weight_hh", lstm_part(&t, dir)?),
            _ => {
                // Input and recurrent biases, end to end.
                let both = t.get(dir)?;
                let half = both.dim(0)? / 2;
                let split = |at: usize| -> Result<Tensor> {
                    let b = both.narrow(0, at, half)?.unsqueeze(1)?;
                    Ok(lstm_part(&b.unsqueeze(0)?, 0)?.squeeze(1)?)
                };
                put("bias_ih", split(0)?);
                put("bias_hh", split(half)?);
            }
        }
    }
    Ok(())
}

/// Every weight by Kokoro's name, and the sizes they imply.
pub(crate) fn read(bytes: &[u8]) -> Result<(Config, HashMap<String, Tensor>)> {
    let (named, layers) = names(bytes)?;
    Ok((sizes(&named, layers)?, named))
}

/// Every weight by Kokoro's name, sized by a config of its own.
pub(crate) fn weights(bytes: &[u8]) -> Result<HashMap<String, Tensor>> {
    Ok(names(bytes)?.0)
}

/// Every weight by Kokoro's name, and how many times ALBERT runs.
fn names(bytes: &[u8]) -> Result<(HashMap<String, Tensor>, usize)> {
    let mut graph = None;
    for field in fields(bytes) {
        if let (7, Value::Bytes(b)) = field? {
            graph = Some(b);
        }
    }
    let graph = graph.ok_or_else(|| Error::Load("no model graph in the file".into()))?;
    let mut stores = HashMap::new();
    let mut nodes = Vec::new();
    for field in fields(graph) {
        match field? {
            (1, Value::Bytes(b)) => nodes.push(node(b)?),
            (5, Value::Bytes(b)) => {
                let (name, s) = stored(b)?;
                stores.insert(name, s);
            }
            _ => {}
        }
    }
    let mut weights = widen(&stores, &readers(&nodes))?;
    drop(stores);
    // A cast or quantised copy stands for the weight it came from.
    let mut alias: HashMap<String, String> = HashMap::new();
    for name in weights.keys() {
        alias.insert(format!("{name}{QUANTISED}"), name.clone());
    }
    for n in nodes.iter().filter(|n| n.op == "Cast") {
        if let (Some(i), Some(o)) = (n.inputs.first(), n.outputs.first())
            && weights.contains_key(i)
        {
            alias.insert(o.clone(), i.clone());
        }
    }
    let mut named = HashMap::new();
    // Norms a file stores once for several modules: (module's name, stored).
    let mut shared: Vec<(String, String)> = Vec::new();
    let mut layers = 0;
    for node in &nodes {
        // A cast only renames a weight; its readers name it.
        if node.op == "Cast" {
            continue;
        }
        let module = module_of(&node.name);
        let op = node.op.as_str();
        let product = matches!(op, "MatMul" | "MatMulInteger");
        if product
            && module
                .as_deref()
                .is_some_and(|m| m.ends_with("attention.query"))
        {
            layers += 1;
        }
        for (slot, input) in node.inputs.iter().enumerate() {
            let from = alias.get(input).map_or(input.as_str(), String::as_str);
            let matmul = product && slot == 1;
            let own = own(from);
            if let (Some(own), Some(module), "InstanceNormalization", 1 | 2) =
                (own.as_deref(), module.as_deref(), op, slot)
            {
                let part = if slot == 1 { "weight" } else { "bias" };
                shared.push((format!("{module}.{part}"), own.to_owned()));
            }
            // Named weights wait for the end unless stored `[in, out]`.
            if !weights.contains_key(from) || (own.is_some() && !matmul) {
                continue;
            }
            if let Some(own) = own
                && let Some(w) = weights.remove(from)
            {
                named.insert(own, tensor(w)?.t()?.contiguous()?);
                continue;
            }
            let Some(module) = module.as_deref() else {
                continue;
            };
            let name = match (op, slot) {
                ("LSTM" | "DynamicQuantizeLSTM", 1..=3) => {
                    if let Some(w) = weights.remove(from) {
                        lstm(module, slot, w, op != "LSTM", &mut named)?;
                    }
                    continue;
                }
                _ if matmul => format!("{module}.weight"),
                ("Conv" | "ConvTranspose" | "ConvInteger" | "Gemm", 1) => {
                    format!("{module}.weight")
                }
                ("Conv" | "ConvTranspose" | "Gemm", 2) => format!("{module}.bias"),
                ("LayerNormalization", 1 | 2) => {
                    let own = module.ends_with("LayerNorm");
                    let names = if own {
                        ["weight", "bias"]
                    } else {
                        ["gamma", "beta"]
                    };
                    let part = names.get(slot - 1).copied().unwrap_or_default();
                    format!("{module}.{part}")
                }
                _ => continue,
            };
            // Taken only once named, so a reader that can't name it leaves it.
            let Some(w) = weights.remove(from) else {
                continue;
            };
            if let Entry::Vacant(e) = named.entry(name) {
                let t = tensor(w)?;
                e.insert(if matmul { t.t()?.contiguous()? } else { t });
            }
        }
    }
    // The rest of the named weights, as stored.
    for (name, w) in weights {
        if let Some(own) = own(&name)
            && !named.contains_key(&own)
        {
            named.insert(own, tensor(w)?);
        }
    }
    for (name, own) in shared {
        if !named.contains_key(&name)
            && let Some(t) = named.get(&own).cloned()
        {
            named.insert(name, t);
        }
    }
    Ok((named, layers))
}

/// Every float or quantised weight, widened to `f32` by its stored name.
fn widen(stores: &HashMap<String, Stored<'_>>, by: &Readers) -> Result<HashMap<String, Weight>> {
    let mut out = HashMap::new();
    for (name, s) in stores {
        let Some(base) = name.strip_suffix(QUANTISED) else {
            // A quantised weight's scale and zero point are read with it.
            let of = name
                .strip_suffix("_scale")
                .or(name.strip_suffix("_zero_point"));
            if of.is_some_and(|b| stores.contains_key(&format!("{b}{QUANTISED}"))) {
                continue;
            }
            if matches!(s.dtype, FLOAT | FLOAT16)
                && let Some(values) = values(name, s)?
            {
                let dims = s.dims.clone();
                out.insert(name.clone(), Weight { dims, values });
            }
            continue;
        };
        let part = |key: String| -> Result<Vec<f32>> {
            let s = stores.get(&key).ok_or_else(|| bad(&format!("no {key}")))?;
            values(&key, s)?.ok_or_else(|| bad(&format!("{key} isn't numbers")))
        };
        // Zero points alike may be stored once, named by the readers.
        let mut zero = format!("{base}_zero_point");
        if !stores.contains_key(&zero)
            && let Some(theirs) = by.zero.get(name)
        {
            zero.clone_from(theirs);
        }
        let q = values(name, s)?.ok_or_else(|| bad(&format!("{name} isn't numbers")))?;
        let scale = part(format!("{base}_scale"))?;
        let columns = by.columns.contains(name);
        let values = dequantise(name, &q, &scale, &part(zero)?, columns)?;
        let dims = s.dims.clone();
        out.insert(base.to_owned(), Weight { dims, values });
    }
    Ok(out)
}

/// The model's sizes, read off the shapes of its weights.
fn sizes(t: &HashMap<String, Tensor>, layers: usize) -> Result<Config> {
    let dim = |name: &str, axis: usize| -> Result<usize> {
        let t = t
            .get(name)
            .ok_or_else(|| Error::Load(format!("the model file has no {name}")))?;
        Ok(t.dim(axis)?)
    };
    let count =
        |name: &dyn Fn(usize) -> String| (0..).take_while(|&i| t.contains_key(&name(i))).count();
    let hidden = dim("bert.encoder.embedding_hidden_mapping_in.weight", 0)?;
    let plbert = Plbert {
        vocab_size: dim("bert.embeddings.word_embeddings.weight", 0)?,
        hidden_size: hidden,
        num_attention_heads: (hidden / HEAD).max(1),
        num_hidden_layers: if layers == 0 { 12 } else { layers },
        intermediate_size: dim(
            "bert.encoder.albert_layer_groups.0.albert_layers.0.ffn.weight",
            0,
        )?,
        max_position_embeddings: dim("bert.embeddings.position_embeddings.weight", 0)?,
        embedding_size: dim("bert.embeddings.word_embeddings.weight", 1)?,
        type_vocab_size: dim("bert.embeddings.token_type_embeddings.weight", 0)?,
        hidden_act: Activation::GeluNew,
    };
    let ups = |i: usize| format!("decoder.generator.ups.{i}.weight");
    let stages = count(&ups);
    let kernels = (0..stages)
        .map(|i| dim(&ups(i), 2))
        .collect::<Result<Vec<_>>>()?;
    let block = |i: usize| format!("decoder.generator.resblocks.{i}.convs1.0.weight");
    let per = count(&block) / stages.max(1);
    let block_kernels = (0..per)
        .map(|j| dim(&block(j), 2))
        .collect::<Result<Vec<_>>>()?;
    let dilations = (0..per)
        .map(|j| {
            let conv = |n: usize| format!("decoder.generator.resblocks.{j}.convs1.{n}.weight");
            DILATIONS.iter().take(count(&conv)).copied().collect()
        })
        .collect();
    let n_fft = dim("decoder.generator.conv_post.weight", 0)?.saturating_sub(2);
    let istftnet = Istft {
        upsample_rates: kernels.iter().map(|k| k / 2).collect(),
        upsample_kernel_sizes: kernels,
        upsample_initial_channel: dim(&ups(0), 0)?,
        resblock_kernel_sizes: block_kernels,
        resblock_dilation_sizes: dilations,
        gen_istft_n_fft: n_fft,
        gen_istft_hop_size: n_fft / 4,
    };
    Ok(Config {
        hidden_dim: dim("text_encoder.embedding.weight", 1)?,
        style_dim: dim("predictor.text_encoder.lstms.1.fc.weight", 1)?,
        n_token: dim("text_encoder.embedding.weight", 0)?,
        n_layer: count(&|i| format!("text_encoder.cnn.{i}.0.weight")),
        max_dur: dim("predictor.duration_proj.linear_layer.weight", 0)?,
        text_encoder_kernel_size: dim("text_encoder.cnn.0.0.weight", 2)?,
        decoder_dim: dim("decoder.encode.conv1.weight", 0)?,
        plbert,
        istftnet,
        vocab: crate::kitten::vocab(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn halves_widen_exactly() {
        assert_eq!(half(0x3c00), 1.0);
        assert_eq!(half(0xc000), -2.0);
        assert_eq!(half(0x3555), 0.333_251_95);
        assert_eq!(half(0x0001), 5.960_464_5e-8);
        assert_eq!(half(0x8000).to_bits(), (-0.0f32).to_bits());
        assert!(half(0x7c00).is_infinite() && half(0x7e00).is_nan());
    }

    #[test]
    fn quantised_values_come_back_per_tensor_or_per_slice() {
        let q = [0.0, 128.0, 255.0, 10.0];
        let one = dequantise("w", &q, &[0.5], &[128.0], false).unwrap();
        assert_eq!(one, [-64.0, 0.0, 63.5, -59.0]);
        let two = dequantise("w", &q, &[1.0, 2.0], &[0.0, 10.0], false).unwrap();
        assert_eq!(two, [0.0, 128.0, 490.0, 0.0]);
        // `[in, out]` products scale each column, here two of them.
        let cols = dequantise("w", &q, &[1.0, 2.0], &[0.0, 10.0], true).unwrap();
        assert_eq!(cols, [0.0, 236.0, 255.0, 0.0]);
        let shared = dequantise("w", &q, &[1.0, 2.0], &[10.0], false).unwrap();
        assert_eq!(shared, [-10.0, 118.0, 490.0, 0.0]);
        assert!(dequantise("w", &q, &[1.0; 3], &[0.0; 3], false).is_err());
        assert!(dequantise("w", &q, &[1.0], &[], false).is_err());
    }

    #[test]
    fn export_paths_become_the_model_own() {
        let cases = [
            (
                "/bert/encoder/albert_layer_groups.0/albert_layers.0/attention/query_7/MatMul",
                "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.query",
            ),
            (
                "/text_encoder/cnn.0/cnn.0.1/LayerNormalization",
                "text_encoder.cnn.0.1",
            ),
            ("/text_encoder/lstm/LSTM", "text_encoder.lstm"),
            (
                "/text_encoder/lstms.2/LSTM_quant",
                "predictor.text_encoder.lstms.2",
            ),
            ("/lstm/LSTM", "predictor.lstm"),
            ("/shared/LSTM", "predictor.shared"),
            (
                "/duration_proj/linear_layer/MatMul",
                "predictor.duration_proj.linear_layer",
            ),
            (
                "/decoder/generator/m_source/l_linear/MatMul",
                "decoder.generator.m_source.l_linear",
            ),
            ("/bert_encoder/MatMul", "bert_encoder"),
            // Kokoro's own export nests the network in two halves.
            (
                "/encoder/text_encoder/cnn.0/cnn.0.0/Conv_quant",
                "text_encoder.cnn.0.0",
            ),
            ("/encoder/F0.1/pool/ConvTranspose", "predictor.F0.1.pool"),
            ("/encoder/N_proj/Conv_quant", "predictor.N_proj"),
            (
                "/encoder/predictor/text_encoder/lstms.0/LSTM_quant",
                "predictor.text_encoder.lstms.0",
            ),
            (
                "/decoder/decoder/asr_res/asr_res.0/Conv_quant",
                "decoder.asr_res.0",
            ),
        ];
        for (node, module) in cases {
            assert_eq!(module_of(node).as_deref(), Some(module), "{node}");
        }
        assert_eq!(module_of("MatMul_3"), None);
        assert_eq!(module_of("/encoder/Constant"), None);
    }

    fn put(n: u64, out: &mut Vec<u8>) {
        let mut n = n;
        while n >= 0x80 {
            out.push((n as u8) | 0x80);
            n >>= 7;
        }
        out.push(n as u8);
    }

    fn bytes(field: u64, body: &[u8], out: &mut Vec<u8>) {
        put((field << 3) | 2, out);
        put(body.len() as u64, out);
        out.extend_from_slice(body);
    }

    fn int(field: u64, v: u64, out: &mut Vec<u8>) {
        put(field << 3, out);
        put(v, out);
    }

    fn init(name: &str, dims: &[u64], dtype: u64, raw: &[u8]) -> Vec<u8> {
        let mut t = Vec::new();
        dims.iter().for_each(|&d| int(1, d, &mut t));
        int(2, dtype, &mut t);
        bytes(8, name.as_bytes(), &mut t);
        bytes(9, raw, &mut t);
        t
    }

    fn floats(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }

    fn op(name: &str, op: &str, inputs: &[&str], output: &str) -> Vec<u8> {
        let mut n = Vec::new();
        inputs.iter().for_each(|i| bytes(1, i.as_bytes(), &mut n));
        bytes(2, output.as_bytes(), &mut n);
        bytes(3, name.as_bytes(), &mut n);
        bytes(4, op.as_bytes(), &mut n);
        n
    }

    fn model(parts: &[(u64, Vec<u8>)]) -> Vec<u8> {
        let mut g = Vec::new();
        parts.iter().for_each(|(f, b)| bytes(*f, b, &mut g));
        let mut m = Vec::new();
        int(1, 8, &mut m);
        bytes(7, &g, &mut m);
        m
    }

    fn values(t: &Tensor) -> Vec<f32> {
        t.flatten_all().unwrap().to_vec1().unwrap()
    }

    #[test]
    fn weights_take_the_model_names_widened_and_turned() {
        let q = "/bert/encoder/albert_layer_groups.0/albert_layers.0/attention/query";
        let bytes = model(&[
            // Named, quantised to bytes, multiplied as `[in, out]`.
            (
                5,
                init("kmodel.a.fc.weight_quantized", &[2, 1], INT8, &[2, 0xfe]),
            ),
            (
                5,
                init("kmodel.a.fc.weight_scale", &[], FLOAT, &floats(&[0.5])),
            ),
            (5, init("kmodel.a.fc.weight_zero_point", &[], INT8, &[0])),
            (
                1,
                op(
                    "/a/fc/MatMul_quant",
                    "MatMulInteger",
                    &["x", "kmodel.a.fc.weight_quantized"],
                    "y",
                ),
            ),
            // Anonymous, in half precision, through a cast, used twice.
            (
                5,
                init("onnx::MatMul_9", &[1, 2], FLOAT16, &[0, 0x3c, 0, 0xc0]),
            ),
            (1, op("cast", "Cast", &["onnx::MatMul_9"], "w32")),
            (1, op(&format!("{q}/MatMul"), "MatMul", &["x", "w32"], "y1")),
            (
                1,
                op(&format!("{q}_1/MatMul"), "MatMul", &["x", "w32"], "y2"),
            ),
            // Named and kept as stored; constants left out.
            (5, init("kmodel.b.bias", &[2], FLOAT, &floats(&[1.0, 2.0]))),
            (5, init("/Constant_output_0", &[1], FLOAT, &floats(&[3.0]))),
            // One norm stored for two modules.
            (
                5,
                init("kmodel.n.0.norm.weight", &[2], FLOAT, &floats(&[0.5, 0.25])),
            ),
            (
                1,
                op(
                    "/n.0/norm/InstanceNormalization",
                    "InstanceNormalization",
                    &["x", "kmodel.n.0.norm.weight"],
                    "z0",
                ),
            ),
            (
                1,
                op(
                    "/n.1/norm/InstanceNormalization",
                    "InstanceNormalization",
                    &["x", "kmodel.n.0.norm.weight"],
                    "z1",
                ),
            ),
        ]);
        let (named, layers) = names(&bytes).unwrap();
        assert_eq!(layers, 2);
        let fc = &named["a.fc.weight"];
        assert_eq!((fc.dims(), values(fc)), (&[1, 2][..], vec![1.0, -1.0]));
        let qw =
            &named["bert.encoder.albert_layer_groups.0.albert_layers.0.attention.query.weight"];
        assert_eq!((qw.dims(), values(qw)), (&[2, 1][..], vec![1.0, -2.0]));
        assert_eq!(values(&named["b.bias"]), [1.0, 2.0]);
        assert_eq!(values(&named["n.1.norm.weight"]), [0.5, 0.25]);
        assert_eq!(named.len(), 5, "{:?}", named.keys().collect::<Vec<_>>());
        assert!(names(&bytes[..bytes.len() - 3]).is_err());
    }

    #[test]
    fn kokoro_exports_share_zero_points_and_scale_columns() {
        let w = "/encoder/text_encoder/cnn.0/Mul_output_0";
        let fc = "decoder.decoder.encode.norm1.fc.weight";
        let bytes = model(&[
            // A conv weight whose zero point another weight stores.
            (
                5,
                init(&format!("{w}_quantized"), &[2, 1, 1], UINT8, &[3, 5]),
            ),
            (5, init(&format!("{w}_scale"), &[], FLOAT, &floats(&[2.0]))),
            (5, init("/other_zero_point", &[], UINT8, &[1])),
            (
                1,
                op(
                    "/encoder/text_encoder/cnn.0/cnn.0.0/Conv_quant",
                    "ConvInteger",
                    &["x", &format!("{w}_quantized"), "xz", "/other_zero_point"],
                    "y",
                ),
            ),
            // A product scaled per output column.
            (5, init(&format!("{fc}_quantized"), &[1, 2], INT8, &[2, 2])),
            (
                5,
                init(&format!("{fc}_scale"), &[2], FLOAT, &floats(&[1.0, 3.0])),
            ),
            (5, init(&format!("{fc}_zero_point"), &[2], INT8, &[0, 0])),
            (
                1,
                op(
                    "/decoder/decoder/encode/norm1/fc/Gemm_MatMul_quant",
                    "MatMulInteger",
                    &["x", &format!("{fc}_quantized")],
                    "z",
                ),
            ),
            (
                5,
                init("encoder.bert_encoder.bias", &[1], FLOAT, &floats(&[7.0])),
            ),
        ]);
        let named = weights(&bytes).unwrap();
        let conv = &named["text_encoder.cnn.0.0.weight"];
        assert_eq!(
            (conv.dims(), values(conv)),
            (&[2, 1, 1][..], vec![4.0, 8.0])
        );
        let fc = &named["decoder.encode.norm1.fc.weight"];
        assert_eq!((fc.dims(), values(fc)), (&[2, 1][..], vec![2.0, 6.0]));
        assert_eq!(values(&named["bert_encoder.bias"]), [7.0]);
        assert_eq!(named.len(), 3, "{:?}", named.keys().collect::<Vec<_>>());
    }

    #[test]
    fn lstm_gates_reorder_and_biases_split() {
        // One direction, hidden 1: rows are the gates i, o, f, c.
        let w = Weight {
            dims: vec![1, 4, 1],
            values: vec![1.0, 2.0, 3.0, 4.0],
        };
        let mut out = HashMap::new();
        lstm("m", 1, w, false, &mut out).unwrap();
        let ih: Vec<f32> = out["m.weight_ih_l0"]
            .flatten_all()
            .unwrap()
            .to_vec1()
            .unwrap();
        assert_eq!(ih, [1.0, 3.0, 4.0, 2.0]);
        let b = Weight {
            dims: vec![1, 8],
            values: (1..=8).map(|v| v as f32).collect(),
        };
        lstm("m", 3, b, false, &mut out).unwrap();
        let hh: Vec<f32> = out["m.bias_hh_l0"].to_vec1().unwrap();
        assert_eq!(hh, [5.0, 7.0, 8.0, 6.0]);
        // Quantised exports keep `[dirs, inputs, gates]`.
        let q = Weight {
            dims: vec![1, 1, 4],
            values: vec![1.0, 2.0, 3.0, 4.0],
        };
        lstm("q", 2, q, true, &mut out).unwrap();
        let hh: Vec<f32> = out["q.weight_hh_l0"]
            .flatten_all()
            .unwrap()
            .to_vec1()
            .unwrap();
        assert_eq!(hh, [1.0, 3.0, 4.0, 2.0]);
    }
}
