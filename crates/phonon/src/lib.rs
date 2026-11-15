//! Gradium's Phonon ported to candle: text in, 24 kHz speech out.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod config;
mod layers;
mod lm;
mod mimi;
mod noise;
mod normalize;
mod text;
mod tokenizer;
mod weights;

use std::path::Path;

use candle_core::{Device, Tensor};

pub use config::Config;
pub use noise::Noise;
pub use normalize::normalize;
pub use text::{CHUNK_TOKENS, Chunk, prepare};
pub use tokenizer::Tokenizer;

use layers::Cache;
use lm::FlowLm;
use mimi::Mimi;
use weights::Weights;

/// Samples per second of everything the model says.
pub const SAMPLE_RATE: u32 = 24_000;

/// The sampling temperature the reference runtime uses.
pub const TEMPERATURE: f32 = 0.3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A file is missing, unreadable or not the expected kind.
    Load(String),
    /// The text or voice can't be spoken.
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

/// A voice, run through the model once so each run starts from it.
#[derive(Clone)]
pub struct Voice {
    primed: Cache,
}

impl Voice {
    /// Prompt positions the voice fills.
    pub fn frames(&self) -> usize {
        self.primed.len()
    }
}

/// The reference's duration delta: 0 by default, below 0 faster.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pace(pub f32);

/// What a run made: its latents and the step the model chose to end at.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    /// One `ldim` row a frame, in the flow head's own scale.
    pub latents: Vec<Vec<f32>>,
    /// The first frame whose end logit crossed the threshold.
    pub ended: Option<usize>,
}

/// A loaded checkpoint: the flow model, Mimi and the tokenizer.
pub struct Phonon {
    config: Config,
    lm: FlowLm,
    mimi: Mimi,
    tokenizer: Tokenizer,
}

impl Phonon {
    /// Reads the config, the weights (GGUF or safetensors) and the tokenizer.
    pub fn load(config: &Path, weights: &Path, tokenizer: &Path) -> Result<Self> {
        let read =
            |p: &Path| std::fs::read(p).map_err(|e| Error::Load(format!("{}: {e}", p.display())));
        let config = Config::parse(&read(config)?)?;
        let tokenizer = Tokenizer::parse(&read(tokenizer)?)?;
        let w = Weights::read(weights)?;
        Ok(Self {
            lm: FlowLm::load(&w, &config)?,
            mimi: Mimi::load(&w, &config)?,
            config,
            tokenizer,
        })
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn tokenizer(&self) -> &Tokenizer {
        &self.tokenizer
    }

    /// A voice file: `speaker_wavs`, `[1, 512, frames]`, made for this model.
    pub fn voice(&self, path: &Path) -> Result<Voice> {
        let bytes =
            std::fs::read(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
        self.voice_from(&bytes)
    }

    /// A voice file already in memory.
    pub fn voice_from(&self, bytes: &[u8]) -> Result<Voice> {
        self.check_kind(bytes)?;
        let w = Weights::from_safetensors(bytes)?;
        let latents = w.any("speaker_wavs")?;
        let latents = match latents.dims() {
            [1, _, _] => latents.squeeze(0)?,
            [_, _] => latents,
            dims => return Err(Error::Load(format!("a voice of shape {dims:?}"))),
        };
        let rows = self.lm.speaker(&latents.t()?.contiguous()?)?;
        let mut primed = self.lm.cache();
        self.lm.prompt(&rows, &mut primed)?;
        Ok(Voice { primed })
    }

    /// Refuses a voice made for another checkpoint, when the file says.
    fn check_kind(&self, bytes: &[u8]) -> Result<()> {
        let kinds = &self.config.embeddings_ext;
        let Some(kind) = voice_kind(bytes) else {
            return Ok(());
        };
        match kinds.is_empty() || kinds.contains(&kind) {
            true => Ok(()),
            false => Err(Error::Load(format!("a voice for {kind}, not this model"))),
        }
    }

    /// `text` read as English and planned as the model's runs.
    pub fn chunks(&self, text: &str) -> Vec<Chunk> {
        let text = normalize(text);
        text::chunks(&self.tokenizer, &text, self.config.frame_rate())
    }

    /// The latents of one run in `voice` at `pace`, noise drawn from `noise`.
    pub fn run(&self, voice: &Voice, chunk: &Chunk, pace: Pace, noise: &mut Noise) -> Result<Run> {
        let mut latents = Vec::new();
        let ended = self.generate(voice, chunk, pace, noise, |latent| {
            latents.push(latent.to_vec());
            Ok(true)
        })?;
        Ok(Run { latents, ended })
    }

    /// Hands `each` every latent of a run until it says stop; the end frame.
    fn generate(
        &self,
        voice: &Voice,
        chunk: &Chunk,
        pace: Pace,
        noise: &mut Noise,
        mut each: impl FnMut(&[f32]) -> Result<bool>,
    ) -> Result<Option<usize>> {
        let conditions = self.lm.conditions(pace.0)?;
        let mut cache = voice.primed.clone();
        self.lm.prompt(&self.lm.text(&chunk.tokens)?, &mut cache)?;
        let mut latent = self.lm.bos.clone();
        let mut ended = None;
        let mut left: Option<usize> = None;
        let mut fresh = vec![0.0; self.lm.ldim];
        for frame in 0..chunk.frame_budget {
            noise.fill(&mut fresh);
            let (next, eos) = self.lm.step(&latent, &fresh, &conditions, &mut cache)?;
            if !each(&next)? {
                break;
            }
            if eos > self.config.eos_threshold && left.is_none() {
                left = Some(chunk.frames_after_eos);
                ended = Some(frame);
            }
            match left.as_mut() {
                Some(0) => break,
                Some(n) => *n -= 1,
                None => {}
            }
            latent = next;
        }
        Ok(ended)
    }

    /// A decoder at the start of a run.
    pub fn decoder(&self) -> Result<Decoder<'_>> {
        Ok(Decoder {
            stream: self.mimi.stream()?,
            lm: &self.lm,
        })
    }

    /// Audio for a whole run's latents.
    pub fn decode(&self, latents: &[Vec<f32>]) -> Result<Vec<f32>> {
        self.decoder()?.push(latents)
    }

    /// One run in `voice`, in growing blocks; `out` returns false to stop.
    pub fn say(
        &self,
        voice: &Voice,
        chunk: &Chunk,
        pace: Pace,
        noise: &mut Noise,
        mut out: impl FnMut(Vec<f32>) -> bool,
    ) -> Result<()> {
        let mut decoder = self.decoder()?;
        let mut pending: Vec<Vec<f32>> = Vec::new();
        let mut block = FIRST_BLOCK;
        let mut going = true;
        self.generate(voice, chunk, pace, noise, |latent| {
            pending.push(latent.to_vec());
            if pending.len() >= block {
                going = out(decoder.push(&pending)?);
                pending.clear();
                block = (block * 2).min(LAST_BLOCK);
            }
            Ok(going)
        })?;
        if going && !pending.is_empty() {
            out(decoder.push(&pending)?);
        }
        Ok(())
    }
}

/// Frames in the first block [`Phonon::say`] decodes: 240 ms of speech.
const FIRST_BLOCK: usize = 3;
/// The most frames a block grows to: two seconds.
const LAST_BLOCK: usize = 25;

/// Mimi's decoder partway through a run; any split of it sounds alike.
pub struct Decoder<'a> {
    stream: mimi::Stream<'a>,
    lm: &'a FlowLm,
}

impl Decoder<'_> {
    /// Audio for the next latents, in the flow head's scale.
    pub fn push(&mut self, latents: &[Vec<f32>]) -> Result<Vec<f32>> {
        let ldim = self.lm.ldim;
        let mut flat = Vec::with_capacity(latents.len() * ldim);
        for row in latents {
            let scaled = row.iter().zip(&self.lm.std).zip(&self.lm.mean);
            flat.extend(scaled.map(|((x, s), m)| x * s + m));
        }
        let t = Tensor::from_vec(flat, (latents.len(), ldim), &Device::Cpu)?;
        self.stream.push(&t)
    }
}

/// A voice file's `sig@epoch`, from its safetensors header.
fn voice_kind(bytes: &[u8]) -> Option<String> {
    let len = u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?) as usize;
    let header: serde_json::Value = serde_json::from_slice(bytes.get(8..8 + len)?).ok()?;
    let meta = header.get("__metadata__")?;
    let sig = meta.get("sig")?.as_str()?;
    let epoch = meta.get("epoch")?.as_str()?;
    Some(format!("{sig}@{epoch}"))
}
