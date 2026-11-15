//! A one-layer BART that spells unknown words in phonemes, as misaki would.

use std::collections::HashMap;

use crate::{Dialect, Error};

const D: usize = 128;
const FFN: usize = 1024;
const MAX_POSITIONS: usize = 64;
/// BART's learned positions start two rows in.
const POSITION_OFFSET: usize = 2;
const LN_EPS: f32 = 1e-5;

type VecD = [f32; D];

const PAD: usize = 0;
const BOS: usize = 1;
const EOS: usize = 2;
const SPECIALS: usize = 4;

/// Longer words are cut, leaving room for the start and end markers.
const MAX_WORD_CHARS: usize = MAX_POSITIONS - 2;

/// Each checkpoint's vocabulary, id by position; the first four are specials.
fn vocabulary(dialect: Dialect) -> (&'static str, &'static str) {
    match dialect {
        Dialect::American => (
            "____AIOWYbdfhijklmnpstuvwz'-.BCDEFGHJKLMNPQRSTUVXZacegoqrxy",
            "____AIOWYbdfhijklmnpstuvwzæðŋɑɔəɛɜɡɪɹɾʃʊʌʒʔʤʧˈˌθᵊᵻ",
        ),
        Dialect::British => (
            "____AIQWYabdfhijklmnpstuvwz'-.BCDEFGHJKLMNOPRSTUVXZcegoqrxy",
            "____AIQWYabdfhijklmnpstuvwzðŋɑɒɔəɛɜɡɪɹʃʊʌʒʤʧˈˌːθᵊ",
        ),
    }
}

struct Linear {
    weight: Vec<f32>,
    bias: Vec<f32>,
    in_dim: usize,
}

impl Linear {
    fn forward_into(&self, x: &[f32], out: &mut [f32]) {
        let rows = self.weight.chunks_exact(self.in_dim);
        for ((o, &b), row) in out.iter_mut().zip(&self.bias).zip(rows) {
            *o = b + dot(row, x);
        }
    }

    fn forward_d(&self, x: &[f32]) -> VecD {
        let mut out = [0.0; D];
        self.forward_into(x, &mut out);
        out
    }
}

struct LayerNorm {
    weight: Vec<f32>,
    bias: Vec<f32>,
}

impl LayerNorm {
    fn forward_in_place(&self, x: &mut [f32]) {
        let n = x.len() as f32;
        let mean = x.iter().sum::<f32>() / n;
        let var = x.iter().map(|v| (v - mean) * (v - mean)).sum::<f32>() / n;
        let inv = 1.0 / (var + LN_EPS).sqrt();
        for ((v, w), b) in x.iter_mut().zip(&self.weight).zip(&self.bias) {
            *v = (*v - mean) * inv * w + b;
        }
    }
}

struct Attention {
    q: Linear,
    k: Linear,
    v: Linear,
    out: Linear,
}

impl Attention {
    fn project_kv(&self, xs: &[VecD]) -> (Vec<VecD>, Vec<VecD>) {
        let keys = xs.iter().map(|x| self.k.forward_d(x)).collect();
        let values = xs.iter().map(|x| self.v.forward_d(x)).collect();
        (keys, values)
    }

    fn attend_one(&self, query: &VecD, keys: &[VecD], values: &[VecD]) -> VecD {
        let scale = (D as f32).powf(-0.5);
        let mut q = self.q.forward_d(query);
        for v in &mut q {
            *v *= scale;
        }
        let mut scores = [0.0f32; MAX_POSITIONS];
        let count = keys.len().min(MAX_POSITIONS);
        let active = scores.get_mut(..count).unwrap_or_default();
        for (slot, k) in active.iter_mut().zip(keys) {
            *slot = dot(&q, k);
        }
        softmax_in_place(active);
        let mut mixed = [0.0f32; D];
        for (&w, v) in active.iter().zip(values) {
            for (m, &x) in mixed.iter_mut().zip(v) {
                *m += w * x;
            }
        }
        self.out.forward_d(&mixed)
    }

    fn forward(&self, queries: &[VecD], keys_values: &[VecD]) -> Vec<VecD> {
        let (keys, values) = self.project_kv(keys_values);
        queries
            .iter()
            .map(|x| self.attend_one(x, &keys, &values))
            .collect()
    }
}

struct FeedForward {
    fc1: Linear,
    fc2: Linear,
}

impl FeedForward {
    fn forward(&self, x: &VecD) -> VecD {
        let mut hidden = [0.0f32; FFN];
        self.fc1.forward_into(x, &mut hidden);
        for v in &mut hidden {
            *v = gelu(*v);
        }
        self.fc2.forward_d(&hidden)
    }
}

struct Embeddings {
    positions: Vec<f32>,
    norm: LayerNorm,
}

impl Embeddings {
    fn embed_one(&self, tokens: &[f32], id: usize, pos: usize) -> VecD {
        let token = row(tokens, id);
        let position = row(&self.positions, pos + POSITION_OFFSET);
        let mut h = [0.0f32; D];
        for ((slot, &t), &p) in h.iter_mut().zip(token).zip(position) {
            *slot = t + p;
        }
        self.norm.forward_in_place(&mut h);
        h
    }

    fn embed(&self, tokens: &[f32], ids: &[usize]) -> Vec<VecD> {
        ids.iter()
            .enumerate()
            .map(|(pos, &id)| self.embed_one(tokens, id, pos))
            .collect()
    }
}

/// Row `i` of a matrix `D` wide.
fn row(matrix: &[f32], i: usize) -> &[f32] {
    matrix.get(i * D..(i + 1) * D).unwrap_or_default()
}

pub(crate) struct NeuralG2p {
    shared: Vec<f32>,
    final_logits_bias: Vec<f32>,
    encoder_embeddings: Embeddings,
    encoder_attn: Attention,
    encoder_attn_norm: LayerNorm,
    encoder_ffn: FeedForward,
    encoder_ffn_norm: LayerNorm,
    decoder_embeddings: Embeddings,
    decoder_self_attn: Attention,
    decoder_self_attn_norm: LayerNorm,
    decoder_cross_attn: Attention,
    decoder_cross_attn_norm: LayerNorm,
    decoder_ffn: FeedForward,
    decoder_ffn_norm: LayerNorm,
    grapheme_ids: HashMap<char, usize>,
    phonemes: Vec<char>,
}

impl NeuralG2p {
    /// The checkpoint for `dialect`, from its safetensors bytes.
    pub(crate) fn from_bytes(bytes: &[u8], dialect: Dialect) -> Result<Self, Error> {
        let mut t = Tensors::parse(bytes)?;
        let linear = |t: &mut Tensors, name: &str, in_dim: usize, out_dim: usize| {
            Ok::<_, Error>(Linear {
                weight: t.take(&format!("{name}.weight"), in_dim * out_dim)?,
                bias: t.take(&format!("{name}.bias"), out_dim)?,
                in_dim,
            })
        };
        let norm = |t: &mut Tensors, name: &str| {
            Ok::<_, Error>(LayerNorm {
                weight: t.take(&format!("{name}.weight"), D)?,
                bias: t.take(&format!("{name}.bias"), D)?,
            })
        };
        let attention = |t: &mut Tensors, name: &str| {
            Ok::<_, Error>(Attention {
                q: linear(t, &format!("{name}.q_proj"), D, D)?,
                k: linear(t, &format!("{name}.k_proj"), D, D)?,
                v: linear(t, &format!("{name}.v_proj"), D, D)?,
                out: linear(t, &format!("{name}.out_proj"), D, D)?,
            })
        };
        let ffn = |t: &mut Tensors, layer: &str| {
            Ok::<_, Error>(FeedForward {
                fc1: linear(t, &format!("{layer}.fc1"), D, FFN)?,
                fc2: linear(t, &format!("{layer}.fc2"), FFN, D)?,
            })
        };
        let embeddings = |t: &mut Tensors, stack: &str| {
            Ok::<_, Error>(Embeddings {
                positions: t.take(
                    &format!("model.{stack}.embed_positions.weight"),
                    (MAX_POSITIONS + POSITION_OFFSET) * D,
                )?,
                norm: norm(t, &format!("model.{stack}.layernorm_embedding"))?,
            })
        };

        let final_logits_bias = t.take_any("final_logits_bias")?;
        let vocab_size = final_logits_bias.len();
        let (graphemes, phonemes) = vocabulary(dialect);
        if graphemes.chars().count().max(phonemes.chars().count()) > vocab_size {
            return Err(Error::Data("g2p model: vocabulary too small".to_owned()));
        }
        let enc = "model.encoder.layers.0";
        let dec = "model.decoder.layers.0";
        let model = Self {
            shared: t.take("model.shared.weight", vocab_size * D)?,
            final_logits_bias,
            encoder_embeddings: embeddings(&mut t, "encoder")?,
            encoder_attn: attention(&mut t, &format!("{enc}.self_attn"))?,
            encoder_attn_norm: norm(&mut t, &format!("{enc}.self_attn_layer_norm"))?,
            encoder_ffn: ffn(&mut t, enc)?,
            encoder_ffn_norm: norm(&mut t, &format!("{enc}.final_layer_norm"))?,
            decoder_embeddings: embeddings(&mut t, "decoder")?,
            decoder_self_attn: attention(&mut t, &format!("{dec}.self_attn"))?,
            decoder_self_attn_norm: norm(&mut t, &format!("{dec}.self_attn_layer_norm"))?,
            decoder_cross_attn: attention(&mut t, &format!("{dec}.encoder_attn"))?,
            decoder_cross_attn_norm: norm(&mut t, &format!("{dec}.encoder_attn_layer_norm"))?,
            decoder_ffn: ffn(&mut t, dec)?,
            decoder_ffn_norm: norm(&mut t, &format!("{dec}.final_layer_norm"))?,
            grapheme_ids: graphemes
                .chars()
                .enumerate()
                .skip(SPECIALS)
                .map(|(i, c)| (c, i))
                .collect(),
            phonemes: phonemes.chars().collect(),
        };
        if let Some(name) = t.remaining() {
            return Err(Error::Data(format!("g2p model: unexpected tensor {name}")));
        }
        Ok(model)
    }

    /// `None` when no letter of `word` is in the vocabulary.
    pub(crate) fn phonemize(&self, word: &str) -> Option<String> {
        let letters = fold(word)
            .chars()
            .filter_map(|c| self.grapheme_ids.get(&c).copied())
            .take(MAX_WORD_CHARS)
            .collect::<Vec<_>>();
        if letters.is_empty() {
            return None;
        }
        let ids: Vec<usize> = std::iter::once(BOS)
            .chain(letters)
            .chain(std::iter::once(EOS))
            .collect();
        let memory = self.encode(&ids);
        let out: String = self
            .decode_greedy(&memory)
            .into_iter()
            .filter(|&id| id >= SPECIALS)
            .filter_map(|id| self.phonemes.get(id))
            .collect();
        (!out.is_empty()).then_some(out)
    }

    fn encode(&self, ids: &[usize]) -> Vec<VecD> {
        let mut h = self.encoder_embeddings.embed(&self.shared, ids);
        let attended = self.encoder_attn.forward(&h, &h);
        for (row, d) in h.iter_mut().zip(&attended) {
            add_norm(row, d, &self.encoder_attn_norm);
        }
        for row in &mut h {
            let fed = self.encoder_ffn.forward(row);
            add_norm(row, &fed, &self.encoder_ffn_norm);
        }
        h
    }

    fn decode_greedy(&self, memory: &[VecD]) -> Vec<usize> {
        let (cross_k, cross_v) = self.decoder_cross_attn.project_kv(memory);
        let mut self_k = Vec::with_capacity(MAX_POSITIONS);
        let mut self_v = Vec::with_capacity(MAX_POSITIONS);
        let mut out = Vec::new();
        let mut current = BOS;
        for pos in 0..MAX_POSITIONS.saturating_sub(1) {
            let mut h = self
                .decoder_embeddings
                .embed_one(&self.shared, current, pos);
            self_k.push(self.decoder_self_attn.k.forward_d(&h));
            self_v.push(self.decoder_self_attn.v.forward_d(&h));
            let attended = self.decoder_self_attn.attend_one(&h, &self_k, &self_v);
            add_norm(&mut h, &attended, &self.decoder_self_attn_norm);
            let crossed = self.decoder_cross_attn.attend_one(&h, &cross_k, &cross_v);
            add_norm(&mut h, &crossed, &self.decoder_cross_attn_norm);
            let fed = self.decoder_ffn.forward(&h);
            add_norm(&mut h, &fed, &self.decoder_ffn_norm);

            let next = self
                .final_logits_bias
                .iter()
                .enumerate()
                .map(|(id, bias)| (id, dot(row(&self.shared, id), &h) + bias))
                .max_by(|a, b| a.1.total_cmp(&b.1))
                .map_or(EOS, |(id, _)| id);
            if next == EOS || next == PAD {
                break;
            }
            out.push(next);
            current = next;
        }
        out
    }
}

fn add_norm(row: &mut VecD, delta: &VecD, norm: &LayerNorm) {
    for (v, &x) in row.iter_mut().zip(delta) {
        *v += x;
    }
    norm.forward_in_place(row);
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn softmax_in_place(scores: &mut [f32]) {
    let max = scores.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for s in scores.iter_mut() {
        *s = (*s - max).exp();
        sum += *s;
    }
    for s in scores.iter_mut() {
        *s /= sum;
    }
}

/// The exact GELU BART uses, not the tanh approximation.
fn gelu(x: f32) -> f32 {
    0.5 * x * (1.0 + libm::erff(x / std::f32::consts::SQRT_2))
}

/// Accented Latin letters onto the plain ones the vocabulary has.
fn fold(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
            'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => 'A',
            'è' | 'é' | 'ê' | 'ë' => 'e',
            'È' | 'É' | 'Ê' | 'Ë' => 'E',
            'ì' | 'í' | 'î' | 'ï' => 'i',
            'Ì' | 'Í' | 'Î' | 'Ï' => 'I',
            'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => 'o',
            'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => 'O',
            'ù' | 'ú' | 'û' | 'ü' => 'u',
            'Ù' | 'Ú' | 'Û' | 'Ü' => 'U',
            'ñ' => 'n',
            'Ñ' => 'N',
            'ç' => 'c',
            'Ç' => 'C',
            'ý' | 'ÿ' => 'y',
            '’' | '‘' | 'ʼ' => '\'',
            '‐' | '‑' | '–' => '-',
            other => other,
        })
        .collect()
}

/// A safetensors file of `F32` tensors, by name.
struct Tensors {
    tensors: HashMap<String, Vec<f32>>,
}

impl Tensors {
    fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let bad = |why: &str| Error::Data(format!("g2p model: {why}"));
        let len: [u8; 8] = bytes
            .get(..8)
            .and_then(|b| b.try_into().ok())
            .ok_or_else(|| bad("truncated"))?;
        let header_end = usize::try_from(u64::from_le_bytes(len))
            .ok()
            .and_then(|n| n.checked_add(8))
            .ok_or_else(|| bad("truncated"))?;
        let header = bytes.get(8..header_end).ok_or_else(|| bad("truncated"))?;
        let data = bytes.get(header_end..).unwrap_or_default();
        let header: HashMap<String, serde_json::Value> =
            serde_json::from_slice(header).map_err(|e| bad(&e.to_string()))?;

        let mut tensors = HashMap::new();
        for (name, meta) in header {
            if name == "__metadata__" {
                continue;
            }
            if meta.get("dtype").and_then(|d| d.as_str()) != Some("F32") {
                return Err(bad(&format!("{name} is not F32")));
            }
            let offset = |i: usize| {
                let offsets = meta.get("data_offsets")?.as_array()?;
                usize::try_from(offsets.get(i)?.as_u64()?).ok()
            };
            let (Some(start), Some(end)) = (offset(0), offset(1)) else {
                return Err(bad(&format!("{name} has no offsets")));
            };
            let raw = data
                .get(start..end)
                .filter(|r| r.len().is_multiple_of(4))
                .ok_or_else(|| bad(&format!("{name} is out of range")))?;
            let values = raw
                .as_chunks::<4>()
                .0
                .iter()
                .copied()
                .map(f32::from_le_bytes)
                .collect();
            tensors.insert(name, values);
        }
        Ok(Self { tensors })
    }

    fn take(&mut self, name: &str, len: usize) -> Result<Vec<f32>, Error> {
        let values = self.take_any(name)?;
        if values.len() != len {
            let got = values.len();
            return Err(Error::Data(format!(
                "g2p model: {name} has {got} values, not {len}"
            )));
        }
        Ok(values)
    }

    fn take_any(&mut self, name: &str) -> Result<Vec<f32>, Error> {
        self.tensors
            .remove(name)
            .ok_or_else(|| Error::Data(format!("g2p model: no tensor {name}")))
    }

    fn remaining(&self) -> Option<&str> {
        self.tensors.keys().next().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vocabularies_fold_accents_and_skip_specials() {
        assert_eq!(fold("Café–naïve’s"), "Cafe-naive's");
        for dialect in [Dialect::American, Dialect::British] {
            let (graphemes, phonemes) = vocabulary(dialect);
            assert!(graphemes.starts_with("____") && phonemes.starts_with("____"));
        }
    }

    #[test]
    fn damaged_weights_are_an_error() {
        let header = br#"{"final_logits_bias":{"dtype":"F32","shape":[1],"data_offsets":[0,4]}}"#;
        let mut file = (header.len() as u64).to_le_bytes().to_vec();
        file.extend(header);
        file.extend(1f32.to_le_bytes());
        assert!(NeuralG2p::from_bytes(&file, Dialect::American).is_err());
        assert!(
            NeuralG2p::from_bytes(file.get(..10).unwrap_or_default(), Dialect::American).is_err()
        );
        assert!(NeuralG2p::from_bytes(&[], Dialect::British).is_err());
    }
}
