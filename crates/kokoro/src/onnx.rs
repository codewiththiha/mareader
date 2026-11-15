//! An ONNX export of the network, read as Kokoro's named `f32` weights.

mod stream;

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

use candle_core::{Device, Tensor};

use mareader_nn_core::wire::{Value, take, text, varint};

#[cfg(test)]
use self::stream::half;
use self::stream::{Stream, TensorIndex, decode_values, dequantise_in_place, scan_graph};
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

/// Every weight by Kokoro's name, and the sizes they imply, streamed from disk.
pub(crate) fn read_file(path: &Path) -> Result<(Config, HashMap<String, Tensor>)> {
    let file = File::open(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
    let (named, layers) = names_stream(BufReader::with_capacity(64 << 10, file))?;
    Ok((sizes(&named, layers)?, named))
}

/// Every weight by Kokoro's name, streamed from disk.
pub(crate) fn weights_file(path: &Path) -> Result<HashMap<String, Tensor>> {
    let file = File::open(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
    Ok(names_stream(BufReader::with_capacity(64 << 10, file))?.0)
}

/// Every weight by Kokoro's name, sized by a config of its own.
#[cfg(test)]
pub(crate) fn weights(bytes: &[u8]) -> Result<HashMap<String, Tensor>> {
    Ok(names(bytes)?.0)
}

#[cfg(test)]
fn names(bytes: &[u8]) -> Result<(HashMap<String, Tensor>, usize)> {
    names_stream(std::io::Cursor::new(bytes))
}

enum WeightSource {
    Plain(String),
    Quantised(String),
}

enum Action {
    OwnTranspose(String),
    Lstm {
        module: String,
        slot: usize,
        transposed: bool,
    },
    Named {
        name: String,
        transpose: bool,
    },
    OwnPlain(String),
}

struct Plan {
    actions: Vec<(String, Action)>,
    shared: Vec<(String, String)>,
    leftover: Vec<String>,
    layers: usize,
}

/// Collects every float or quantised weight key before reading bulk data.
fn weight_sources(stores: &HashMap<String, TensorIndex>) -> HashMap<String, WeightSource> {
    let mut sources = HashMap::new();
    for (name, idx) in stores {
        if let Some(base) = name.strip_suffix(QUANTISED) {
            sources.insert(base.to_owned(), WeightSource::Quantised(name.clone()));
            continue;
        }
        let of = name
            .strip_suffix("_scale")
            .or(name.strip_suffix("_zero_point"));
        if of.is_some_and(|b| stores.contains_key(&format!("{b}{QUANTISED}"))) {
            continue;
        }
        if matches!(idx.dtype, FLOAT | FLOAT16) {
            sources
                .entry(name.clone())
                .or_insert_with(|| WeightSource::Plain(name.clone()));
        }
    }
    sources
}

fn op_action(op: &str, module: &str, slot: usize, matmul: bool) -> Option<Action> {
    Some(match (op, slot) {
        ("LSTM" | "DynamicQuantizeLSTM", 1..=3) => Action::Lstm {
            module: module.to_owned(),
            slot,
            transposed: op != "LSTM",
        },
        _ if matmul => Action::Named {
            name: format!("{module}.weight"),
            transpose: true,
        },
        ("Conv" | "ConvTranspose" | "ConvInteger" | "Gemm", 1) => Action::Named {
            name: format!("{module}.weight"),
            transpose: false,
        },
        ("Conv" | "ConvTranspose" | "Gemm", 2) => Action::Named {
            name: format!("{module}.bias"),
            transpose: false,
        },
        ("LayerNormalization", 1 | 2) => {
            let names = if module.ends_with("LayerNorm") {
                ["weight", "bias"]
            } else {
                ["gamma", "beta"]
            };
            let part = names.get(slot - 1).copied().unwrap_or_default();
            Action::Named {
                name: format!("{module}.{part}"),
                transpose: false,
            }
        }
        _ => return None,
    })
}

/// Maps every weight key to its target Kokoro name before streaming bytes.
fn plan_names(nodes: &[Node], sources: &HashMap<String, WeightSource>) -> Plan {
    let mut pending: HashSet<String> = sources.keys().cloned().collect();
    // A cast or quantised copy stands for the weight it came from.
    let mut alias: HashMap<String, String> = HashMap::new();
    for name in &pending {
        alias.insert(format!("{name}{QUANTISED}"), name.clone());
    }
    for n in nodes.iter().filter(|n| n.op == "Cast") {
        if let (Some(i), Some(o)) = (n.inputs.first(), n.outputs.first())
            && pending.contains(i)
        {
            alias.insert(o.clone(), i.clone());
        }
    }
    let mut actions = Vec::new();
    // Norms a file stores once for several modules: (module's name, stored).
    let mut shared = Vec::new();
    let mut layers = 0;
    for node in nodes {
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
            if !pending.contains(from) || (own.is_some() && !matmul) {
                continue;
            }
            if let Some(own) = own {
                pending.remove(from);
                actions.push((from.to_owned(), Action::OwnTranspose(own)));
                continue;
            }
            let Some(action) = module
                .as_deref()
                .and_then(|m| op_action(op, m, slot, matmul))
            else {
                continue;
            };
            pending.remove(from);
            actions.push((from.to_owned(), action));
        }
    }
    let mut leftover = Vec::new();
    for name in pending {
        if let Some(own) = own(&name) {
            actions.push((name, Action::OwnPlain(own)));
        } else {
            leftover.push(name);
        }
    }
    Plan {
        actions,
        shared,
        leftover,
        layers,
    }
}

fn load_weight<R: Read + Seek>(
    stream: &mut Stream<R>,
    stores: &HashMap<String, TensorIndex>,
    by: &Readers,
    source: &WeightSource,
    base: &str,
    scratch: &mut Vec<u8>,
) -> Result<Weight> {
    match source {
        WeightSource::Plain(name) => {
            let idx = stores.get(name).ok_or_else(|| bad(&format!("no {name}")))?;
            let values = decode_values(stream, name, idx, scratch)?
                .ok_or_else(|| bad(&format!("{name} isn't numbers")))?;
            Ok(Weight {
                dims: idx.dims.clone(),
                values,
            })
        }
        WeightSource::Quantised(name) => {
            let idx = stores.get(name).ok_or_else(|| bad(&format!("no {name}")))?;
            let mut part = |stream: &mut Stream<R>, key: &str| -> Result<Vec<f32>> {
                let s = stores.get(key).ok_or_else(|| bad(&format!("no {key}")))?;
                decode_values(stream, key, s, scratch)?
                    .ok_or_else(|| bad(&format!("{key} isn't numbers")))
            };
            let mut zero_key = format!("{base}_zero_point");
            if !stores.contains_key(&zero_key)
                && let Some(theirs) = by.zero.get(name)
            {
                zero_key.clone_from(theirs);
            }
            let scale = part(stream, &format!("{base}_scale"))?;
            let zero = part(stream, &zero_key)?;
            let mut values = decode_values(stream, name, idx, scratch)?
                .ok_or_else(|| bad(&format!("{name} isn't numbers")))?;
            dequantise_in_place(name, &mut values, &scale, &zero, by.columns.contains(name))?;
            Ok(Weight {
                dims: idx.dims.clone(),
                values,
            })
        }
    }
}

/// Every weight by Kokoro's name, streamed one tensor at a time.
fn names_stream<R: Read + Seek>(reader: R) -> Result<(HashMap<String, Tensor>, usize)> {
    let mut stream = Stream::new(reader)?;
    let (nodes, stores) = scan_graph(&mut stream)?;
    let by = readers(&nodes);
    let sources = weight_sources(&stores);
    let plan = plan_names(&nodes, &sources);
    drop(nodes);
    let mut named = HashMap::with_capacity(plan.actions.len());
    let mut scratch = Vec::new();
    for (base, action) in plan.actions {
        let Some(source) = sources.get(&base) else {
            continue;
        };
        let w = load_weight(&mut stream, &stores, &by, source, &base, &mut scratch)?;
        match action {
            Action::OwnTranspose(own) => {
                named.insert(own, tensor(w)?.t()?.contiguous()?);
            }
            Action::Lstm {
                module,
                slot,
                transposed,
            } => lstm(&module, slot, w, transposed, &mut named)?,
            Action::Named { name, transpose } => {
                if let Entry::Vacant(e) = named.entry(name) {
                    let t = tensor(w)?;
                    e.insert(if transpose { t.t()?.contiguous()? } else { t });
                }
            }
            Action::OwnPlain(own) => {
                if let Entry::Vacant(e) = named.entry(own) {
                    e.insert(tensor(w)?);
                }
            }
        }
    }
    for base in plan.leftover {
        if let Some(source) = sources.get(&base) {
            let _ = load_weight(&mut stream, &stores, &by, source, &base, &mut scratch)?;
        }
    }
    for (name, own) in plan.shared {
        if !named.contains_key(&name)
            && let Some(t) = named.get(&own).cloned()
        {
            named.insert(name, t);
        }
    }
    Ok((named, plan.layers))
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

    fn dequantise(
        name: &str,
        q: &[f32],
        scale: &[f32],
        zero: &[f32],
        columns: bool,
    ) -> Result<Vec<f32>> {
        let mut out = q.to_vec();
        dequantise_in_place(name, &mut out, scale, zero, columns)?;
        Ok(out)
    }

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
        let path = std::env::temp_dir().join(format!("mareader-onnx-{}.onnx", std::process::id()));
        std::fs::write(&path, &bytes).unwrap();
        let streamed = weights_file(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(streamed.len(), named.len());
        for (k, v) in &named {
            assert_eq!(values(&streamed[k]), values(v), "{k}");
        }
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
