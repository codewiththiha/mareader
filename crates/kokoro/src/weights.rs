//! Reading weight files: PyTorch, safetensors, GGUF and ONNX builds; voices.

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use candle_core::pickle::{Object, Stack, TensorInfo};
use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Shape, Tensor};
use candle_nn::Init;
use candle_nn::var_builder::SimpleBackend;

use crate::Error;

/// The model file's top-level parts.
const PARTS: [&str; 5] = [
    "bert",
    "bert_encoder",
    "decoder",
    "predictor",
    "text_encoder",
];

/// Every weight by name, `module.` wrappers dropped, as `f32`.
pub fn read_model(path: &Path) -> Result<HashMap<String, Tensor>, Error> {
    let mut all = HashMap::new();
    if path.extension().is_some_and(|e| e == "gguf") {
        return read_gguf(path);
    }
    if path.extension().is_some_and(|e| e == "onnx") {
        return crate::onnx::weights_file(path);
    }
    if path.extension().is_some_and(|e| e == "safetensors") {
        for (name, t) in candle_core::safetensors::load(path, &Device::Cpu)? {
            all.insert(name, t.to_dtype(DType::F32)?);
        }
        return Ok(all);
    }
    // Opened once: candle's reader reopens the archive for every tensor.
    let file = File::open(path).map_err(|e| bad(path, e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| bad(path, e))?;
    for part in PARTS {
        let infos = candle_core::pickle::read_pth_tensor_info(path, false, Some(part))
            .map_err(|e| Error::Load(format!("{} ({part}): {e}", path.display())))?;
        for info in infos {
            let name = format!("{part}.{}", info.name).replace(".module.", ".");
            let t = stored(&mut zip, &info).map_err(|e| bad(path, format!("{name}: {e}")))?;
            all.insert(name, t.to_dtype(DType::F32)?);
        }
    }
    Ok(all)
}

/// One tensor's data from the open archive, laid out as its info says.
fn stored<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    info: &TensorInfo,
) -> std::result::Result<Tensor, String> {
    let layout = &info.layout;
    let fortran = layout.is_fortran_contiguous();
    if !layout.is_contiguous() && !fortran {
        return Err(format!("a strided tensor {layout:?}"));
    }
    let shape = layout.shape();
    let size = shape.elem_count() * info.dtype.size_in_bytes();
    // Candle's pickle reader keeps the offset in bytes, not elements.
    let skip = layout.start_offset();
    let entry = zip.by_name(&info.path).map_err(|e| e.to_string())?;
    let mut rest = entry.take((skip + size) as u64);
    std::io::copy(&mut rest.by_ref().take(skip as u64), &mut std::io::sink())
        .map_err(|e| e.to_string())?;
    let mut data = Vec::with_capacity(size);
    rest.read_to_end(&mut data).map_err(|e| e.to_string())?;
    if data.len() != size {
        return Err("the data is cut short".into());
    }
    let t = Tensor::from_raw_buffer(&data, info.dtype, shape.dims(), &Device::Cpu);
    let t = t.map_err(|e| e.to_string())?;
    if fortran && shape.rank() > 1 {
        // Stored with the last axis slowest: read reversed, then turned.
        let reversed: Vec<usize> = shape.dims().iter().rev().copied().collect();
        let axes: Vec<usize> = (0..shape.rank()).rev().collect();
        let turned = t.reshape(reversed).and_then(|t| t.permute(axes));
        return turned.map_err(|e| e.to_string());
    }
    Ok(t)
}

/// A GGUF build's weights, renamed to the `.pth` layout and widened.
fn read_gguf(path: &Path) -> Result<HashMap<String, Tensor>, Error> {
    let mut file = BufReader::new(File::open(path).map_err(|e| bad(path, e))?);
    let content = gguf_file::Content::read(&mut file).map_err(|e| bad(path, e))?;
    let mut all = HashMap::with_capacity(content.tensor_infos.len());
    for name in content.tensor_infos.keys() {
        let t = content.tensor(&mut file, name, &Device::Cpu)?;
        let t = t.dequantize(&Device::Cpu)?.to_dtype(DType::F32)?;
        all.insert(pth_name(name), t);
    }
    Ok(all)
}

/// GGUF name prefixes and the `.pth` ones they stand for.
const RENAMES: [(&str, &str); 22] = [
    ("bert.embd.tok.", "bert.embeddings.word_embeddings."),
    ("bert.embd.pos.", "bert.embeddings.position_embeddings."),
    ("bert.embd.tt.", "bert.embeddings.token_type_embeddings."),
    ("bert.embd.ln.", "bert.embeddings.LayerNorm."),
    (
        "bert.embd_proj.",
        "bert.encoder.embedding_hidden_mapping_in.",
    ),
    (
        "bert.attn_q.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.query.",
    ),
    (
        "bert.attn_k.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.key.",
    ),
    (
        "bert.attn_v.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.value.",
    ),
    (
        "bert.attn_o.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.dense.",
    ),
    (
        "bert.attn_ln.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.LayerNorm.",
    ),
    (
        "bert.ffn_ln.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.full_layer_layer_norm.",
    ),
    (
        "bert.ffn_up.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.ffn.",
    ),
    (
        "bert.ffn_down.",
        "bert.encoder.albert_layer_groups.0.albert_layers.0.ffn_output.",
    ),
    ("bert_proj.", "bert_encoder."),
    ("pred.dur_proj.", "predictor.duration_proj.linear_layer."),
    ("pred.", "predictor."),
    ("dec.gen.m_source.", "decoder.generator.m_source.l_linear."),
    ("dec.asr_res.", "decoder.asr_res.0."),
    ("dec.gen.", "decoder.generator."),
    ("dec.", "decoder."),
    ("text_enc.embd.", "text_encoder.embedding."),
    ("text_enc.", "text_encoder."),
];

/// A GGUF tensor's name as the `.pth` file has it.
fn pth_name(gguf: &str) -> String {
    let renamed = RENAMES
        .iter()
        .find_map(|(from, to)| gguf.strip_prefix(from).map(|rest| format!("{to}{rest}")))
        .unwrap_or_else(|| gguf.to_owned());
    let parts: Vec<&str> = renamed.split('.').collect();
    let mut out: Vec<String> = Vec::with_capacity(parts.len() + 2);
    let mut i = 0;
    while let Some(&part) = parts.get(i) {
        let next = parts.get(i + 1).copied().unwrap_or_default();
        let index = next.parse::<usize>().ok();
        match (part, index) {
            // Each duration layer is an LSTM, then a norm.
            ("dur_enc", Some(k)) => {
                let lstm = parts.get(i + 2) == Some(&"lstm");
                out.extend(["text_encoder".into(), "lstms".into()]);
                if lstm {
                    out.push((2 * k).to_string());
                } else {
                    out.extend([(2 * k + 1).to_string(), "fc".into()]);
                }
                i += 3;
                continue;
            }
            ("adain1" | "adain2", Some(_)) => {
                out.extend([part.into(), next.into(), "fc".into()]);
                i += 2;
                continue;
            }
            ("adain1" | "adain2", None) => {
                let n = part.strip_prefix("adain").unwrap_or_default();
                out.extend([format!("norm{n}"), "fc".into()]);
            }
            ("conv" | "ln", _)
                if out.len() >= 2 && out.get(out.len() - 2).is_some_and(|c| c == "cnn") =>
            {
                out.push(if part == "conv" { "0" } else { "1" }.into());
            }
            _ => out.push(part.into()),
        }
        i += 1;
    }
    out.join(".")
}

/// Named weights that reshape when only size-one axes differ.
pub(crate) struct Lenient(pub HashMap<String, Tensor>);

impl SimpleBackend for Lenient {
    fn get(
        &self,
        s: Shape,
        name: &str,
        _: Init,
        dtype: DType,
        dev: &Device,
    ) -> candle_core::Result<Tensor> {
        let t = self.get_unchecked(name, dtype, dev)?;
        if t.shape() == &s {
            return Ok(t);
        }
        let squeezed = |d: &[usize]| d.iter().copied().filter(|&n| n != 1).collect::<Vec<_>>();
        if squeezed(t.dims()) != squeezed(s.dims()) {
            return Err(candle_core::Error::UnexpectedShape {
                msg: format!("shape mismatch for {name}"),
                expected: s,
                got: t.shape().clone(),
            });
        }
        t.reshape(s)
    }

    fn get_unchecked(&self, name: &str, dtype: DType, dev: &Device) -> candle_core::Result<Tensor> {
        let t = self
            .0
            .get(name)
            .ok_or_else(|| candle_core::Error::CannotFindTensor {
                path: name.to_owned(),
            })?;
        t.to_device(dev)?.to_dtype(dtype)
    }

    fn contains_tensor(&self, name: &str) -> bool {
        self.0.contains_key(name)
    }
}

fn bad(path: &Path, what: impl std::fmt::Display) -> Error {
    Error::Load(format!("{}: {what}", path.display()))
}

/// A `.pt` file holding one bare tensor, as a voice pack does.
pub(crate) fn read_tensor(path: &Path) -> Result<Tensor, Error> {
    let file = File::open(path).map_err(|e| bad(path, e))?;
    let mut zip = zip::ZipArchive::new(BufReader::new(file)).map_err(|e| bad(path, e))?;
    let pickle = zip
        .file_names()
        .filter_map(Result::ok)
        .find(|n| n.ends_with("data.pkl"))
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| bad(path, "no data.pkl inside"))?;
    let mut stack = Stack::empty();
    {
        let entry = zip.by_name(&pickle).map_err(|e| bad(path, e))?;
        stack.read_loop(&mut BufReader::new(entry))?;
    }
    let dir = Path::new(pickle.strip_suffix(".pkl").unwrap_or(&pickle));
    let info = stack
        .finalize()?
        .into_tensor_info(Object::Unicode(String::new()), dir)?
        .ok_or_else(|| bad(path, "not a single tensor"))?;
    let t = stored(&mut zip, &info).map_err(|e| bad(path, e))?;
    Ok(t.to_dtype(DType::F32)?)
}

#[cfg(test)]
mod tests {
    use candle_core::{DType, Device, Tensor};
    use candle_nn::VarBuilder;

    use super::*;

    #[test]
    fn gguf_names_map_back_to_the_pth_layout() {
        let cases = [
            (
                "bert.attn_q.weight",
                "bert.encoder.albert_layer_groups.0.albert_layers.0.attention.query.weight",
            ),
            (
                "bert.embd.tok.weight",
                "bert.embeddings.word_embeddings.weight",
            ),
            ("bert_proj.bias", "bert_encoder.bias"),
            (
                "pred.dur_enc.1.lstm.weight_ih_l0_reverse",
                "predictor.text_encoder.lstms.2.weight_ih_l0_reverse",
            ),
            (
                "pred.dur_enc.2.adaln.weight",
                "predictor.text_encoder.lstms.5.fc.weight",
            ),
            (
                "pred.dur_proj.weight",
                "predictor.duration_proj.linear_layer.weight",
            ),
            ("pred.F0.1.adain2.bias", "predictor.F0.1.norm2.fc.bias"),
            ("pred.F0.1.pool.weight", "predictor.F0.1.pool.weight"),
            ("dec.asr_res.weight", "decoder.asr_res.0.weight"),
            (
                "dec.gen.m_source.weight",
                "decoder.generator.m_source.l_linear.weight",
            ),
            (
                "dec.gen.resblocks.4.adain1.2.weight",
                "decoder.generator.resblocks.4.adain1.2.fc.weight",
            ),
            (
                "dec.gen.noise_res.1.alpha2.0",
                "decoder.generator.noise_res.1.alpha2.0",
            ),
            (
                "dec.decode.3.conv1x1.weight",
                "decoder.decode.3.conv1x1.weight",
            ),
            ("text_enc.cnn.2.ln.gamma", "text_encoder.cnn.2.1.gamma"),
            ("text_enc.cnn.0.conv.weight", "text_encoder.cnn.0.0.weight"),
            ("text_enc.embd.weight", "text_encoder.embedding.weight"),
            ("text_enc.lstm.bias_hh_l0", "text_encoder.lstm.bias_hh_l0"),
        ];
        for (gguf, pth) in cases {
            assert_eq!(pth_name(gguf), pth, "{gguf}");
        }
    }

    #[test]
    fn lenient_weights_restore_dropped_unit_axes_only() {
        let cpu = Device::Cpu;
        let mut all = HashMap::new();
        all.insert(
            "a".to_owned(),
            Tensor::zeros(256, DType::F32, &cpu).unwrap(),
        );
        all.insert(
            "b".to_owned(),
            Tensor::zeros((2, 3), DType::F32, &cpu).unwrap(),
        );
        let vb = VarBuilder::from_backend(Box::new(Lenient(all)), DType::F32, cpu);
        assert_eq!(vb.get((1, 256, 1), "a").unwrap().dims(), [1, 256, 1]);
        assert!(vb.get((3, 2), "b").is_err());
        assert!(vb.get(6, "b").is_err());
        assert!(vb.get(1, "missing").is_err());
    }
}
