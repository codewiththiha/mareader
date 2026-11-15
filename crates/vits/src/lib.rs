//! Piper's VITS voices ported to candle: phonemes in, speech out.
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
mod dense;
mod duration;
mod encoder;
mod flow;
mod fused;
mod generator;
mod layers;
mod noise;
mod onnx;
mod phonemes;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::time::Instant;

use candle_core::{Device, Tensor};
pub(crate) use mareader_nn_core::simd;
use serde::Deserialize;

pub use config::{Audio, Config, Espeak};
pub use phonemes::{Style, espeak_like};

use duration::DurationPredictor;
use encoder::TextEncoder;
use flow::Flow;
use generator::Generator;
use layers::Weights;
use noise::Noise;

/// Most phoneme ids taken at once; a sentence is far fewer.
pub const MAX_IDS: usize = 2_000;
/// Most frames one phoneme may last.
const MAX_FRAMES_PER_ID: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A file is missing, unreadable or not the expected kind.
    Load(String),
    /// The phonemes or settings can't be spoken.
    Input(String),
    /// The maths failed part way.
    Compute(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Load(why) => write!(f, "couldn't load the voice: {why}"),
            Self::Input(why) => write!(f, "couldn't speak this: {why}"),
            Self::Compute(why) => write!(f, "the voice failed: {why}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<candle_core::Error> for Error {
    fn from(e: candle_core::Error) -> Self {
        Self::Compute(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Piper's three knobs, named as in a voice's config.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Scales {
    /// How much the sound varies from the mean; 0 is flat.
    #[serde(rename = "noise_scale")]
    pub noise: f32,
    /// Time per phoneme: above 1 is slower.
    #[serde(rename = "length_scale")]
    pub length: f32,
    /// How much the timing varies; 0 is the same every time.
    pub noise_w: f32,
}

impl Scales {
    fn check(self) -> Result<Self> {
        let ok = (0.0..=5.0).contains(&self.noise)
            && (0.0..=5.0).contains(&self.noise_w)
            && self.length > 0.0
            && self.length <= 10.0;
        ok.then_some(self)
            .ok_or_else(|| Error::Input(format!("the scales {self:?} are out of range")))
    }
}

/// Milliseconds spent in each step, in the order first met.
#[derive(Clone, Debug)]
pub struct Laps {
    pub steps: Vec<(String, f64)>,
    since: Instant,
}

impl Laps {
    fn new() -> Self {
        Self {
            steps: Vec::new(),
            since: Instant::now(),
        }
    }

    /// Books the time since the last lap to `name`, adding to its earlier laps.
    pub(crate) fn lap(&mut self, name: &str) {
        let ms = self.since.elapsed().as_secs_f64() * 1000.0;
        self.since = Instant::now();
        match self.steps.iter_mut().find(|(n, _)| n == name) {
            Some((_, total)) => *total += ms,
            None => self.steps.push((name.to_owned(), ms)),
        }
    }
}

pub struct Vits {
    encoder: TextEncoder,
    duration: DurationPredictor,
    flow: Flow,
    generator: Generator,
    /// One row per speaker, in multi-speaker voices.
    speakers: Option<Tensor>,
}

impl Vits {
    /// A Piper voice's `.onnx` file.
    pub fn load(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).map_err(|e| Error::Load(format!("{}: {e}", path.display())))?;
        Self::from_onnx(&bytes)
    }

    pub fn from_onnx(bytes: &[u8]) -> Result<Self> {
        let w = Weights::new(onnx::read(bytes)?);
        Ok(Self {
            encoder: TextEncoder::load(&w)?,
            duration: DurationPredictor::load(&w)?,
            flow: Flow::load(&w)?,
            generator: Generator::load(&w)?,
            speakers: w.get("emb_g.weight").ok(),
        })
    }

    /// Speakers to choose from; 1 for a single voice.
    pub fn speakers(&self) -> usize {
        self.speakers
            .as_ref()
            .and_then(|t| t.dims().first().copied())
            .unwrap_or(1)
    }

    /// Samples in −1..1 at the config's rate, for ids from [`Config::ids`].
    pub fn speak(&self, ids: &[u32], speaker: usize, scales: Scales) -> Result<Vec<f32>> {
        Ok(self.spoken(ids, speaker, scales)?.audio)
    }

    /// As [`Self::speak`], with the time each step took.
    pub fn speak_timed(
        &self,
        ids: &[u32],
        speaker: usize,
        scales: Scales,
    ) -> Result<(Vec<f32>, Laps)> {
        let spoken = self.spoken(ids, speaker, scales)?;
        Ok((spoken.audio, spoken.laps))
    }

    /// As [`Self::speak`], with the frames each id is heard for.
    pub fn spoken(&self, ids: &[u32], speaker: usize, scales: Scales) -> Result<Spoken> {
        let mut laps = Laps::new();
        let scales = scales.check()?;
        if ids.is_empty() || ids.len() > MAX_IDS {
            return Err(Error::Input(format!("{} phoneme ids", ids.len())));
        }
        let speaker = match &self.speakers {
            Some(table) if speaker < self.speakers() => {
                Some(table.narrow(0, speaker, 1)?.unsqueeze(2)?)
            }
            Some(_) => return Err(Error::Input(format!("there is no speaker {speaker}"))),
            None => None,
        };
        let mut noise = Noise::for_ids(ids);
        let encoded = self.encoder.forward(ids)?;
        laps.lap("text encoder");
        let len = ids.len();
        let timing = [
            noise.take(len, scales.noise_w),
            noise.take(len, scales.noise_w),
        ];
        let log_frames = self
            .duration
            .forward(&encoded.hidden, speaker.as_ref(), timing)?;
        laps.lap("durations");
        let mut frame_of = Vec::new();
        let mut lengths = Vec::with_capacity(len);
        for (id, log) in (0u32..).zip(log_frames) {
            let frames = (log.exp() * scales.length).ceil();
            let frames = if frames.is_finite() && frames > 0.0 {
                (frames as usize).min(MAX_FRAMES_PER_ID)
            } else {
                0
            };
            frame_of.extend(std::iter::repeat_n(id, frames));
            lengths.push(frames);
        }
        if frame_of.is_empty() {
            return Ok(Spoken {
                audio: Vec::new(),
                frames: lengths,
                laps,
            });
        }
        let frames = frame_of.len();
        let frame_of = Tensor::from_vec(frame_of, frames, &Device::Cpu)?;
        let mean = encoded.mean.index_select(&frame_of, 2)?;
        let spread = encoded.log_spread.index_select(&frame_of, 2)?.exp()?;
        let channels = mean.dim(1)?;
        let draws = noise.take(channels * frames, scales.noise);
        let draws = Tensor::from_vec(draws, (1, channels, frames), &Device::Cpu)?;
        let prior = (mean + (draws * spread)?)?;
        let z = self.flow.inverse(&prior, speaker.as_ref())?;
        laps.lap("flow");
        let audio = self.generator.forward(&z, speaker.as_ref(), &mut laps)?;
        Ok(Spoken {
            audio,
            frames: lengths,
            laps,
        })
    }
}

/// Samples said, and how many frames of them each id took.
pub struct Spoken {
    pub audio: Vec<f32>,
    pub frames: Vec<usize>,
    pub laps: Laps,
}
