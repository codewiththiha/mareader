//! Kokoro-82M ported to candle: phonemes in, speech at 24 kHz out.

mod albert;
mod chunks;
mod config;
mod decoder;
mod fast;
pub mod kitten;
mod layers;
mod noise;
mod onnx;
mod prosody;
mod weights;

use std::collections::HashMap;
use std::path::Path;

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::{Linear, VarBuilder};
pub(crate) use mareader_nn_core::simd;

pub use chunks::Chunk;
pub use config::{Activation, Config, Istft, Plbert};
pub use weights::read_model;

use albert::Albert;
use decoder::{Decoder, Probes};
use noise::Noise;
use prosody::{ProsodyPredictor, TextEncoder};

/// Samples per second of everything the model says.
pub const SAMPLE_RATE: u32 = 24_000;

/// Slowest and fastest speaking rates the model takes.
pub const SPEEDS: std::ops::RangeInclusive<f32> = 0.1..=5.0;

/// The weights file inside a model folder.
pub const WEIGHTS: &str = "kokoro-v1_0.pth";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A file is missing, unreadable or not the expected kind.
    Load(String),
    /// The text or settings can't be spoken.
    Input(String),
    /// The maths failed part way.
    Compute(String),
}

mareader_nn_core::voice_error_display!();

impl std::error::Error for Error {}

impl From<candle_core::Error> for Error {
    fn from(e: candle_core::Error) -> Self {
        Self::Compute(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A voice: one style row per phoneme count it was tuned for.
#[derive(Clone)]
pub struct Voice {
    rows: Tensor,
}

impl Voice {
    /// A voice pack, `voices/<id>.pt` in the model folder.
    pub fn load(path: &Path) -> Result<Self> {
        Self::from_tensor(weights::read_tensor(path)?)
    }

    /// Rows `[n, 256]` or `[n, 1, 256]`.
    pub fn from_tensor(rows: Tensor) -> Result<Self> {
        let rows = match rows.rank() {
            3 => rows.squeeze(1)?,
            _ => rows,
        };
        match rows.dims() {
            [n, _] if *n > 0 => Ok(Self { rows }),
            dims => Err(Error::Load(format!("a voice of shape {dims:?}"))),
        }
    }

    /// The style `[1, 256]` at row `at`, or the last row past the end.
    pub fn row(&self, at: usize) -> Result<Tensor> {
        let last = self.rows.dim(0)?.saturating_sub(1);
        Ok(self.rows.narrow(0, at.min(last), 1)?)
    }

    /// The style `[1, 256]` for a padded run of `tokens`.
    pub fn style(&self, tokens: usize) -> Result<Tensor> {
        let last = self.rows.dim(0)?.saturating_sub(1);
        let row = tokens.saturating_sub(3).min(last).min(509);
        Ok(self.rows.narrow(0, row, 1)?)
    }
}

/// A chunk said: its samples, and each id's frames.
pub struct Timed {
    pub audio: Vec<f32>,
    pub frames: Vec<usize>,
    /// Samples before the pause after it.
    pub voiced: usize,
}

/// Every step of one forward pass, kept for checking against references.
pub struct Trace {
    /// Phoneme encoder output `[1, n, 768]`.
    pub bert: Tensor,
    /// Its projection `[1, 512, n]`.
    pub projected: Tensor,
    /// Duration encoder output `[1, n, 640]`.
    pub encoded: Tensor,
    /// Unrounded frames per phoneme `[1, n]`, speed applied.
    pub raw_durations: Tensor,
    pub durations: Vec<usize>,
    /// Pitch and energy, two per frame: `[1, 2 × frames]`.
    pub pitch: Tensor,
    pub energy: Tensor,
    /// Text encoder output `[1, 512, n]`.
    pub text: Tensor,
    pub audio: Vec<f32>,
    /// The generator's steps by name, when asked for.
    #[doc(hidden)]
    pub probes: Vec<(String, Tensor)>,
    /// Seconds each step took, in order, when asked for; names repeat.
    #[doc(hidden)]
    pub laps: Vec<(String, f64)>,
}

pub struct Kokoro {
    config: Config,
    bert: Albert,
    bert_encoder: Linear,
    predictor: ProsodyPredictor,
    text: TextEncoder,
    decoder: Decoder,
}

impl Kokoro {
    /// A model folder holding `config.json` and the weights.
    pub fn load(dir: &Path) -> Result<Self> {
        Self::from_files(&dir.join("config.json"), &dir.join(WEIGHTS))
    }

    /// `weights` may be the `.pth`, safetensors, GGUF or ONNX build.
    pub fn from_files(config: &Path, weights: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(config).map_err(|e| Error::Load(format!("{}: {e}", config.display())))?;
        let config =
            Config::parse(&bytes).map_err(|e| Error::Load(format!("{}: {e}", config.display())))?;
        Self::from_tensors(config, read_model(weights)?)
    }

    /// An ONNX export of the network, as Kitten's builds are.
    pub fn from_onnx(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
        let (config, tensors) = onnx::read(&bytes)?;
        drop(bytes);
        Self::from_tensors(config, tensors)
    }

    /// Every weight name an ONNX file yields, sorted, for checks.
    #[doc(hidden)]
    pub fn onnx_names(path: &Path) -> Result<Vec<String>> {
        let bytes =
            std::fs::read(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
        let mut names: Vec<String> = onnx::read(&bytes)?.1.into_keys().collect();
        names.sort();
        Ok(names)
    }

    /// Built from named weights, as the model file lays them out.
    pub fn from_tensors(config: Config, tensors: HashMap<String, Tensor>) -> Result<Self> {
        let lenient = Box::new(weights::Lenient(tensors));
        let vb = VarBuilder::from_backend(lenient, DType::F32, Device::Cpu);
        Self::build(config, &vb).map_err(|e| Error::Load(e.to_string()))
    }

    fn build(config: Config, vb: &VarBuilder) -> candle_core::Result<Self> {
        let (hidden, style, depth) = (config.hidden_dim, config.style_dim, config.n_layer);
        let kernel = config.text_encoder_kernel_size;
        let plbert = config.plbert.hidden_size;
        Ok(Self {
            bert: Albert::load(&config.plbert, &vb.pp("bert"))?,
            bert_encoder: candle_nn::linear(plbert, hidden, vb.pp("bert_encoder"))?,
            predictor: ProsodyPredictor::load(
                style,
                hidden,
                depth,
                config.max_dur,
                &vb.pp("predictor"),
            )?,
            text: TextEncoder::load(
                hidden,
                kernel,
                depth,
                config.n_token,
                &vb.pp("text_encoder"),
            )?,
            decoder: Decoder::load(
                (hidden, config.decoder_dim),
                style,
                &config.istftnet,
                &vb.pp("decoder"),
            )?,
            config,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    /// G2P output reduced to the symbols this model knows.
    pub fn normalise(&self, phonemes: &str) -> String {
        self.config.normalise(phonemes)
    }

    /// Phonemes in pieces short enough to speak one at a time.
    pub fn chunks(&self, phonemes: &str) -> Vec<Chunk> {
        let most = self.config.plbert.max_position_embeddings.saturating_sub(2);
        chunks::split(phonemes, &self.config.vocab, most)
    }

    /// As [`Self::chunks`], the first ending near `lead` tokens, sooner heard.
    pub fn chunks_led(&self, phonemes: &str, lead: usize) -> Vec<Chunk> {
        let most = self.config.plbert.max_position_embeddings.saturating_sub(2);
        chunks::split_led(phonemes, &self.config.vocab, lead, most)
    }

    /// One chunk spoken, with its pause after; equal input sounds equal.
    pub fn render(&self, chunk: &Chunk, voice: &Voice, speed: f32) -> Result<Vec<f32>> {
        Ok(self.render_timed(chunk, voice, speed)?.audio)
    }

    /// As [`Self::render`], with the frames each id took, pads included.
    pub fn render_timed(&self, chunk: &Chunk, voice: &Voice, speed: f32) -> Result<Timed> {
        let mut ids = Vec::with_capacity(chunk.ids.len() + 2);
        ids.push(0);
        ids.extend_from_slice(&chunk.ids);
        ids.push(0);
        let style = voice.style(ids.len())?;
        let trace = self.trace(&ids, &style, speed)?;
        let (mut audio, frames) = (trace.audio, trace.durations);
        let voiced = audio.len();
        chunks::pause(&mut audio, SAMPLE_RATE, chunk.pause_ms, speed);
        Ok(Timed {
            audio,
            frames,
            voiced,
        })
    }

    /// Speaks all of `phonemes`; `sink` returns false to stop early.
    pub fn speak(
        &self,
        phonemes: &str,
        voice: &Voice,
        speed: f32,
        mut sink: impl FnMut(Vec<f32>) -> bool,
    ) -> Result<()> {
        let chunks = self.chunks(phonemes);
        if chunks.is_empty() {
            return Err(Error::Input("no phonemes the voice knows".into()));
        }
        for chunk in &chunks {
            if !sink(self.render(chunk, voice, speed)?) {
                break;
            }
        }
        Ok(())
    }

    /// A full pass over padded ids `[0, …, 0]`, every step kept.
    pub fn trace(&self, ids: &[u32], style: &Tensor, speed: f32) -> Result<Trace> {
        self.trace_noise(ids, style, speed, Noise::seed_for(ids), 1.0, false)
    }

    /// As [`Self::trace`] with the source noise's seed and gain chosen.
    #[doc(hidden)]
    pub fn trace_noise(
        &self,
        ids: &[u32],
        style: &Tensor,
        speed: f32,
        seed: u64,
        gain: f32,
        probe: bool,
    ) -> Result<Trace> {
        if !speed.is_finite() || !SPEEDS.contains(&speed) {
            return Err(Error::Input(format!("speed {speed} is outside 0.1 to 5")));
        }
        let mut probes = Probes::on(probe);
        let ids_t = Tensor::new(ids, &Device::Cpu)?.unsqueeze(0)?;
        let sd = self.config.style_dim;
        let s_pred = style.narrow(1, sd, sd)?;
        let s_dec = style.narrow(1, 0, sd)?;
        let bert = self.bert.forward(&ids_t)?;
        let projected = self.bert_encoder.forward(&bert)?.transpose(1, 2)?;
        probes.lap(|| "phoneme encoder".into());
        let encoded = self.predictor.encode(&projected, &s_pred)?;
        let raw_durations = layers::scale(&self.predictor.durations(&encoded)?, 1.0 / speed)?;
        probes.lap(|| "durations".into());
        let durations = rounded(&raw_durations)?;
        let frames: usize = durations.iter().sum();
        let mut picks = Vec::with_capacity(frames);
        for (i, &d) in durations.iter().enumerate() {
            let i = u32::try_from(i).map_err(|_| Error::Input("too many phonemes".into()))?;
            picks.extend(std::iter::repeat_n(i, d));
        }
        let picks = Tensor::from_vec(picks, frames, &Device::Cpu)?;
        let aligned = encoded
            .transpose(1, 2)?
            .contiguous()?
            .index_select(&picks, 2)?;
        let (pitch, energy) = self.predictor.curves(&aligned, &s_pred)?;
        probes.lap(|| "pitch and energy".into());
        let text = self.text.forward(&ids_t)?;
        let asr = text.contiguous()?.index_select(&picks, 2)?;
        probes.lap(|| "text encoder".into());
        let mut noise = Noise::new(seed).with_gain(gain);
        let curves = (&pitch, &energy);
        let audio = self
            .decoder
            .forward(&asr, curves, &s_dec, &mut noise, &mut probes)?;
        Ok(Trace {
            bert,
            projected,
            encoded,
            raw_durations,
            durations,
            pitch,
            energy,
            text,
            audio,
            probes: probes.kept.unwrap_or_default(),
            laps: probes.laps.map(|(_, laps)| laps).unwrap_or_default(),
        })
    }
}

/// Frames per phoneme, rounded, at least one each.
fn rounded(raw: &Tensor) -> Result<Vec<usize>> {
    let raw: Vec<f32> = raw.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
    if raw.iter().any(|d| !d.is_finite()) {
        return Err(Error::Compute(
            "a phoneme's length came out undefined".into(),
        ));
    }
    Ok(raw
        .iter()
        .map(|d| (d + 2.5e-4).round().max(1.0) as usize)
        .collect())
}

#[cfg(test)]
mod tests;
