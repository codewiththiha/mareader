//! Frames to sound: a harmonic source shaped by an inverse-STFT generator.

use std::time::Instant;

use candle_core::{DType, Device, Result, Tensor};
use candle_nn::VarBuilder;

use crate::SAMPLE_RATE;
use crate::config::Istft;
use crate::fast::{self, Then};
use crate::layers::{AdaIn1d, Conv, Conv1d, ConvTranspose1d, leaky_relu, scale, upsample};
use crate::noise::Noise;
use crate::prosody::AdainResBlk1d;

/// Named steps kept from a pass, for checking; off unless asked.
#[derive(Default)]
pub(crate) struct Probes {
    pub(crate) kept: Option<Vec<(String, Tensor)>>,
    /// Seconds each step took, timed from the step before.
    pub(crate) laps: Option<(Instant, Vec<(String, f64)>)>,
}

impl Probes {
    /// Steps kept and timed, or neither.
    pub(crate) fn on(on: bool) -> Self {
        Self {
            kept: on.then(Vec::new),
            laps: on.then(|| (Instant::now(), Vec::new())),
        }
    }

    fn keep(&mut self, name: impl Fn() -> String, t: &Tensor) {
        if let Some(all) = &mut self.kept {
            all.push((name(), t.clone()));
        }
        self.lap(name);
    }

    /// Times the work since the last lap as `name`.
    pub(crate) fn lap(&mut self, name: impl FnOnce() -> String) {
        if let Some((last, laps)) = &mut self.laps {
            laps.push((name(), last.elapsed().as_secs_f64()));
            *last = Instant::now();
        }
    }
}

/// Harmonics above the fundamental in the source.
const HARMONICS: usize = 8;
const SINE_AMP: f32 = 0.1;
const NOISE_STD: f32 = 0.003;
/// Pitch below this, in hertz, counts as unvoiced.
const VOICED: f32 = 10.0;

/// `n` points from `at` stretched to `len`, as linear interpolation.
fn resample_iter(n: usize, len: usize, at: impl Fn(usize) -> f32) -> impl Iterator<Item = f32> {
    let count = if n == 0 { 0 } else { len };
    let last = n.saturating_sub(1);
    let ratio = n as f32 / len.max(1) as f32;
    (0..count).map(move |i| {
        if n == len {
            return at(i);
        }
        if last == 0 {
            return at(0);
        }
        let src = ((i as f32 + 0.5) * ratio - 0.5).clamp(0.0, last as f32);
        let left = src.floor() as usize;
        let frac = src - left as f32;
        let a = at(left);
        let b = at((left + 1).min(last));
        a * (1.0 - frac) + b * frac
    })
}

/// A fixed phase offset per harmonic, as the model's source expects.
fn offset(seed: u64) -> f32 {
    let mut v = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    v = (v ^ (v >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    v = (v ^ (v >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    let v = v ^ (v >> 31);
    ((v >> 40) as u32 as f32) / ((1u32 << 24) as f32)
}

/// Sines at the pitch and its harmonics, noise where unvoiced, merged to one.
struct Source {
    upsampling: usize,
    weight_t: Tensor,
    bias: Option<Tensor>,
}

impl Source {
    fn load(upsampling: usize, vb: &VarBuilder) -> Result<Self> {
        Ok(Self {
            upsampling: upsampling.max(1),
            weight_t: vb.get((1, HARMONICS + 1), "l_linear.weight")?.t()?,
            bias: vb.get(1, "l_linear.bias").ok(),
        })
    }

    /// Pitch per sample `[1, len, 1]` to the source `[1, len, 1]`.
    fn forward(&self, f0: &Tensor, noise: &mut Noise) -> Result<Tensor> {
        let (_, len, _) = f0.dims3()?;
        let f0: Vec<f32> = f0.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let dims = HARMONICS + 1;
        let coarse = (len / self.upsampling).max(1);
        let mut waves = vec![0f32; len * dims];
        fast::by_rows(&mut waves, len, |h, row| {
            let times = (h + 1) as f32;
            let shift = if h > 0 { offset(h as u64) } else { 0.0 };
            let rad = |i: usize| {
                let v = f0.get(i).copied().unwrap_or(0.0);
                let base = (v * times / SAMPLE_RATE as f32).rem_euclid(1.0);
                if h > 0 && i == 0 { base + shift } else { base }
            };
            let mut acc = 0f32;
            let phase: Vec<f32> = resample_iter(f0.len(), coarse, rad)
                .map(|v| {
                    acc += v;
                    acc * std::f32::consts::TAU * self.upsampling as f32
                })
                .collect();
            let at = |i: usize| phase.get(i).copied().unwrap_or(0.0);
            for (slot, p) in row.iter_mut().zip(resample_iter(phase.len(), len, at)) {
                *slot = p.sin() * SINE_AMP;
            }
        });
        let mut sines = vec![0f32; len * dims];
        noise.fill_normal(&mut sines);
        // Voiced samples keep sines under faint noise; the rest is noise.
        for (i, (row, v)) in sines.chunks_exact_mut(dims).zip(&f0).enumerate() {
            let voiced = *v > VOICED;
            let amp = if voiced { NOISE_STD } else { SINE_AMP / 3.0 };
            for (h, slot) in row.iter_mut().enumerate() {
                let sine = waves.get(h * len + i).copied().filter(|_| voiced);
                *slot = sine.unwrap_or(0.0) + *slot * amp;
            }
        }
        let sines = Tensor::from_vec(sines, (1, len, dims), &Device::Cpu)?;
        let merged = sines.broadcast_matmul(&self.weight_t)?;
        let merged = match &self.bias {
            Some(b) => merged.broadcast_add(b)?,
            None => merged,
        };
        merged.tanh()
    }
}

/// A short-time Fourier transform with a Hann window, and its inverse.
struct Stft {
    n_fft: usize,
    hop: usize,
    window: Vec<f32>,
    /// `cos` and `sin` of `2π k n / N`, row `k` after row.
    cos: Vec<f32>,
    sin: Vec<f32>,
    /// Set for Kitten's ONNX files, whose STFT runs on stored kernels.
    kernels: Option<Kernels>,
}

/// What the export adds under each magnitude's square root.
const EXPORT_FLOOR: f32 = 1e-14;

/// Stored STFT kernels, `[bins, n_fft]`: near-empty bins' phases hinge on them.
struct Kernels {
    forward_real: Vec<f32>,
    forward_imag: Vec<f32>,
    backward_real: Vec<f32>,
    backward_imag: Vec<f32>,
}

impl Kernels {
    fn load(bins: usize, n_fft: usize, vb: &VarBuilder) -> Result<Self> {
        let get = |name: &str| -> Result<Vec<f32>> {
            vb.get((bins, 1, n_fft), name)?
                .flatten_all()?
                .to_dtype(DType::F32)?
                .to_vec1()
        };
        Ok(Self {
            forward_real: get("weight_forward_real")?,
            forward_imag: get("weight_forward_imag")?,
            backward_real: get("weight_backward_real")?,
            backward_imag: get("weight_backward_imag")?,
        })
    }
}

/// `atan2` as the export builds it from `Atan` and three selects.
fn export_phase(re: f32, im: f32) -> f32 {
    use std::f32::consts::PI;
    match (re < 0.0, im) {
        (true, 0.0) => PI,
        (true, im) if im > 0.0 => (im / re).atan() + PI,
        (true, im) => (im / re).atan() - PI,
        (false, 0.0) if re == 0.0 => 0.0,
        (false, im) => (im / re).atan(),
    }
}

impl Stft {
    fn new(n_fft: usize, hop: usize) -> Result<Self> {
        if n_fft < 2 || !n_fft.is_multiple_of(2) || hop == 0 {
            candle_core::bail!("an STFT needs an even window of 2 or more, and a hop");
        }
        let n = n_fft as f32;
        let window = (0..n_fft)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n).cos()))
            .collect();
        // Rounded as the reference rounds it: near-empty bins keep its phase.
        let angles: Vec<f32> = (0..n_fft * n_fft)
            .map(|i| 2.0 * std::f32::consts::PI * (i / n_fft) as f32 * (i % n_fft) as f32 / n)
            .collect();
        Ok(Self {
            n_fft,
            hop,
            window,
            cos: angles.iter().map(|a| a.cos()).collect(),
            sin: angles.iter().map(|a| a.sin()).collect(),
            kernels: None,
        })
    }

    fn with_kernels(self, kernels: Kernels) -> Self {
        Self {
            kernels: Some(kernels),
            ..self
        }
    }

    fn bins(&self) -> usize {
        self.n_fft / 2 + 1
    }

    /// `samples` padded by reflection at both ends.
    fn reflected(&self, samples: &[f32]) -> Result<Vec<f32>> {
        let pad = self.n_fft / 2;
        if samples.len() <= pad {
            candle_core::bail!("{} samples is too short for the STFT", samples.len());
        }
        let len = samples.len();
        let mut out = Vec::with_capacity(len + 2 * pad);
        if let (true, Some(&first), Some(&last)) =
            (self.kernels.is_some(), samples.first(), samples.last())
        {
            out.extend(std::iter::repeat_n(first, pad));
            out.extend_from_slice(samples);
            out.extend(std::iter::repeat_n(last, pad));
            return Ok(out);
        }
        let head = samples.iter().skip(1).take(pad).rev();
        let tail = samples.iter().rev().skip(1).take(pad);
        out.extend(head);
        out.extend_from_slice(samples);
        out.extend(tail);
        Ok(out)
    }

    /// Mono `[1, len]` to magnitude and phase, each `[1, bins, frames]`.
    fn transform(&self, wave: &Tensor) -> Result<(Tensor, Tensor)> {
        let samples: Vec<f32> = wave.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let padded = self.reflected(&samples)?;
        let frames = 1 + samples.len() / self.hop;
        let bins = self.bins();
        if let Some(kernels) = &self.kernels {
            return self.transform_stored(kernels, &padded, frames);
        }
        let mut magnitude = vec![0f32; bins * frames];
        let mut phase = vec![0f32; bins * frames];
        let mut frame = vec![0f32; self.n_fft];
        for f in 0..frames {
            let piece = padded.iter().skip(f * self.hop);
            for ((slot, s), w) in frame.iter_mut().zip(piece).zip(&self.window) {
                *slot = s * w;
            }
            let rows = self
                .cos
                .chunks_exact(self.n_fft)
                .zip(self.sin.chunks_exact(self.n_fft));
            for (k, (cos, sin)) in rows.take(bins).enumerate() {
                // Summed from +0: a negative DC frame keeps phase +π, not -π.
                let re = frame.iter().zip(cos).fold(0f32, |a, (s, c)| a + s * c);
                let im = frame.iter().zip(sin).fold(0f32, |a, (s, c)| a - s * c);
                if let Some(m) = magnitude.get_mut(k * frames + f) {
                    *m = (re * re + im * im).sqrt();
                }
                if let Some(p) = phase.get_mut(k * frames + f) {
                    *p = im.atan2(re);
                }
            }
        }
        let shape = (1, bins, frames);
        Ok((
            Tensor::from_vec(magnitude, shape, &Device::Cpu)?,
            Tensor::from_vec(phase, shape, &Device::Cpu)?,
        ))
    }

    /// Magnitude and phase `[1, bins, frames]` back to samples.
    fn inverse(&self, magnitude: &Tensor, phase: &Tensor) -> Result<Vec<f32>> {
        let (_, bins, frames) = magnitude.dims3()?;
        if phase.dims() != magnitude.dims() || bins != self.bins() || frames == 0 {
            candle_core::bail!("magnitude and phase don't fit the inverse STFT");
        }
        let mags: Vec<f32> = magnitude.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        let phases: Vec<f32> = phase.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?;
        if let Some(kernels) = &self.kernels {
            return Ok(self.inverse_stored(kernels, &mags, &phases, frames));
        }
        let n = self.n_fft;
        let half = n / 2;
        let len = n + self.hop * (frames - 1);
        let mut out = vec![0f32; len];
        let mut envelope = vec![0f32; len];
        let mut re = vec![0f32; n];
        let mut im = vec![0f32; n];
        for f in 0..frames {
            // The full spectrum: each bin, and its mirror for a real signal.
            for k in 0..bins {
                let i = k * frames + f;
                let (m, p) = (
                    mags.get(i).copied().unwrap_or(0.0),
                    phases.get(i).copied().unwrap_or(0.0),
                );
                let (r, j) = (m * p.cos(), m * p.sin());
                if let (Some(a), Some(b)) = (re.get_mut(k), im.get_mut(k)) {
                    (*a, *b) = (r, j);
                }
                if k > 0
                    && k < half
                    && let (Some(a), Some(b)) = (re.get_mut(n - k), im.get_mut(n - k))
                {
                    (*a, *b) = (r, -j);
                }
            }
            let at = f * self.hop;
            let slots = out.iter_mut().skip(at).zip(envelope.iter_mut().skip(at));
            for (t, ((slot, env), w)) in slots.zip(&self.window).enumerate() {
                let cos = self.cos.iter().skip(t).step_by(n);
                let sin = self.sin.iter().skip(t).step_by(n);
                let parts = re.iter().zip(&im).zip(cos.zip(sin));
                let value: f32 = parts.map(|((r, j), (c, s))| r * c - j * s).sum();
                *slot += value / n as f32 * w;
                *env += w * w;
            }
        }
        for (v, e) in out.iter_mut().zip(&envelope) {
            if *e > 1e-8 {
                *v /= e;
            }
        }
        match len > 2 * half {
            true => Ok(out.into_iter().skip(half).take(len - 2 * half).collect()),
            false => Ok(out),
        }
    }

    /// [`Self::transform`] on stored kernels, frames taken from `padded`.
    fn transform_stored(
        &self,
        kernels: &Kernels,
        padded: &[f32],
        frames: usize,
    ) -> Result<(Tensor, Tensor)> {
        let (n, bins) = (self.n_fft, self.bins());
        let mut magnitude = vec![0f32; bins * frames];
        let mut phase = vec![0f32; bins * frames];
        let rows = kernels
            .forward_real
            .chunks_exact(n)
            .zip(kernels.forward_imag.chunks_exact(n));
        for (k, (real, imag)) in rows.take(bins).enumerate() {
            for f in 0..frames {
                let piece = padded.iter().skip(f * self.hop).take(n);
                let (re, im) = piece
                    .zip(real.iter().zip(imag))
                    .fold((0f32, 0f32), |(a, b), (s, (r, i))| (a + s * r, b + s * i));
                if let Some(m) = magnitude.get_mut(k * frames + f) {
                    *m = (re * re + im * im + EXPORT_FLOOR).sqrt();
                }
                if let Some(p) = phase.get_mut(k * frames + f) {
                    *p = export_phase(re, im);
                }
            }
        }
        let shape = (1, bins, frames);
        Ok((
            Tensor::from_vec(magnitude, shape, &Device::Cpu)?,
            Tensor::from_vec(phase, shape, &Device::Cpu)?,
        ))
    }

    /// [`Self::inverse`] on stored kernels: frames summed, not normalised.
    fn inverse_stored(
        &self,
        kernels: &Kernels,
        mags: &[f32],
        phases: &[f32],
        frames: usize,
    ) -> Vec<f32> {
        let (n, bins, half) = (self.n_fft, self.bins(), self.n_fft / 2);
        let len = n + self.hop * (frames - 1);
        let mut out = vec![0f32; len];
        let rows = || {
            (kernels.backward_real.chunks_exact(n))
                .zip(kernels.backward_imag.chunks_exact(n))
                .take(bins)
        };
        for f in 0..frames {
            let mut acc = vec![0f32; n];
            for (k, (real, imag)) in rows().enumerate() {
                let i = k * frames + f;
                let m = mags.get(i).copied().unwrap_or(0.0);
                let p = phases.get(i).copied().unwrap_or(0.0);
                let (r, j) = (m * p.cos(), m * p.sin());
                for (a, (br, bi)) in acc.iter_mut().zip(real.iter().zip(imag)) {
                    *a += r * br - j * bi;
                }
            }
            for (slot, a) in out.iter_mut().skip(f * self.hop).take(n).zip(acc) {
                *slot += a;
            }
        }
        match len > 2 * half {
            true => out.into_iter().skip(half).take(len - 2 * half).collect(),
            false => out,
        }
    }
}

/// Dilated convolutions with snake activations and style-led norms.
struct SnakeBlock {
    layers: Vec<SnakeLayer>,
}

/// Each norm is followed by snake, `x + sin²(αx) / α`, with its own `α`s.
struct SnakeLayer {
    norm1: AdaIn1d,
    alpha1: Vec<f32>,
    conv1: Conv1d,
    norm2: AdaIn1d,
    alpha2: Vec<f32>,
    conv2: Conv1d,
}

impl SnakeBlock {
    fn load(
        ch: usize,
        kernel: usize,
        dilations: &[usize],
        style: usize,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let alpha = |name: String| -> Result<Vec<f32>> {
            match vb.get((1, ch, 1), &name) {
                Ok(a) => a.flatten_all()?.to_dtype(DType::F32)?.to_vec1(),
                Err(_) => Ok(vec![1.0; ch]),
            }
        };
        let layers = dilations
            .iter()
            .enumerate()
            .map(|(i, &d)| {
                Ok(SnakeLayer {
                    norm1: AdaIn1d::load(style, ch, &vb.pp("adain1").pp(i))?,
                    alpha1: alpha(format!("alpha1.{i}"))?,
                    conv1: Conv::new(ch, ch, kernel)
                        .padding((kernel * d - d) / 2)
                        .dilation(d)
                        .load(&vb.pp("convs1").pp(i))?,
                    norm2: AdaIn1d::load(style, ch, &vb.pp("adain2").pp(i))?,
                    alpha2: alpha(format!("alpha2.{i}"))?,
                    conv2: Conv::new(ch, ch, kernel)
                        .padding((kernel - 1) / 2)
                        .load(&vb.pp("convs2").pp(i))?,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self { layers })
    }

    fn forward(&self, x: &Tensor, s: &Tensor, probes: &mut Probes) -> Result<Tensor> {
        let mut x = x.clone();
        for l in &self.layers {
            let mut y = x.clone();
            for (norm, alpha, conv) in [
                (&l.norm1, &l.alpha1, &l.conv1),
                (&l.norm2, &l.alpha2, &l.conv2),
            ] {
                y = norm.forward_then(&y, s, Then::Snake(alpha))?;
                probes.lap(|| "snake block: norm and snake".into());
                y = conv.forward(&y)?;
                probes.lap(|| "snake block: conv".into());
            }
            x = (y + x)?;
        }
        Ok(x)
    }
}

/// One upsampling stage: the transposed conv, its source path, its blocks.
struct Stage {
    up: ConvTranspose1d,
    noise_conv: Conv1d,
    noise_res: SnakeBlock,
    blocks: Vec<SnakeBlock>,
}

struct Generator {
    source: Source,
    stft: Stft,
    stages: Vec<Stage>,
    post: Conv1d,
    /// Samples per pitch frame.
    f0_scale: usize,
}

impl Generator {
    fn load(c: &Istft, style: usize, vb: &VarBuilder) -> Result<Self> {
        let f0_scale = c.upsample_rates.iter().product::<usize>() * c.gen_istft_hop_size;
        let n_fft = c.gen_istft_n_fft;
        let count = c.upsample_rates.len();
        let first = c.upsample_initial_channel;
        let rates = c.upsample_rates.iter().zip(&c.upsample_kernel_sizes);
        let stages = rates
            .enumerate()
            .map(|(i, (&u, &k))| {
                let ch = first >> (i + 1);
                let up = Conv::new(first >> i, ch, k)
                    .stride(u)
                    .padding(k.saturating_sub(u) / 2)
                    .load_transposed(&vb.pp("ups").pp(i))?;
                let later: usize = c.upsample_rates.iter().skip(i + 1).product();
                let (noise_conv, kernel) = match i + 1 < count {
                    true => (
                        Conv::new(n_fft + 2, ch, later * 2)
                            .stride(later)
                            .padding(later.div_ceil(2)),
                        7,
                    ),
                    false => (Conv::new(n_fft + 2, ch, 1), 11),
                };
                let kinds = c
                    .resblock_kernel_sizes
                    .iter()
                    .zip(&c.resblock_dilation_sizes);
                let blocks = kinds
                    .enumerate()
                    .map(|(j, (&k, d))| {
                        let at = i * c.resblock_kernel_sizes.len() + j;
                        SnakeBlock::load(ch, k, d, style, &vb.pp("resblocks").pp(at))
                    })
                    .collect::<Result<_>>()?;
                Ok(Stage {
                    up,
                    noise_conv: noise_conv.load(&vb.pp("noise_convs").pp(i))?,
                    noise_res: SnakeBlock::load(
                        ch,
                        kernel,
                        &[1, 3, 5],
                        style,
                        &vb.pp("noise_res").pp(i),
                    )?,
                    blocks,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            source: Source::load(f0_scale, &vb.pp("m_source"))?,
            stft: match vb.contains_tensor("stft.weight_forward_real") {
                true => Stft::new(n_fft, c.gen_istft_hop_size)?.with_kernels(Kernels::load(
                    n_fft / 2 + 1,
                    n_fft,
                    &vb.pp("stft"),
                )?),
                false => Stft::new(n_fft, c.gen_istft_hop_size)?,
            },
            stages,
            post: Conv::new(first >> count, n_fft + 2, 7)
                .padding(3)
                .load(&vb.pp("conv_post"))?,
            f0_scale,
        })
    }

    fn forward(
        &self,
        x: &Tensor,
        s: &Tensor,
        f0: &Tensor,
        noise: &mut Noise,
        probes: &mut Probes,
    ) -> Result<Vec<f32>> {
        probes.keep(|| "x0".into(), x);
        let f0 = upsample(&f0.unsqueeze(1)?, f0.dim(1)? * self.f0_scale)?.transpose(1, 2)?;
        let source = self.source.forward(&f0, noise)?;
        let source = source.transpose(1, 2)?.squeeze(1)?;
        probes.keep(|| "source".into(), &source);
        let (spec, phase) = self.stft.transform(&source)?;
        let har = Tensor::cat(&[&spec, &phase], 1)?;
        probes.keep(|| "har".into(), &har);
        let mut x = x.clone();
        let last = self.stages.len().saturating_sub(1);
        for (i, stage) in self.stages.iter().enumerate() {
            x = leaky_relu(&x, 0.1)?;
            let src = stage.noise_conv.forward(&har)?;
            probes.lap(|| format!("stage{i}.noise_conv"));
            let src = stage.noise_res.forward(&src, s, probes)?;
            probes.keep(|| format!("stage{i}.src"), &src);
            x = stage.up.forward(&x)?;
            probes.lap(|| format!("stage{i}.up"));
            if i == last {
                // Reflection-pad one step at the start.
                x = Tensor::cat(&[&x.narrow(2, 1, 1)?, &x], 2)?;
            }
            let n = x.dim(2)?.min(src.dim(2)?);
            x = (x.narrow(2, 0, n)? + src.narrow(2, 0, n)?)?;
            probes.keep(|| format!("stage{i}.sum"), &x);
            let mut sum: Option<Tensor> = None;
            for block in &stage.blocks {
                let y = block.forward(&x, s, probes)?;
                sum = Some(match sum {
                    Some(prev) => (prev + y)?,
                    None => y,
                });
            }
            if let Some(sum) = sum {
                x = scale(&sum, 1.0 / stage.blocks.len() as f32)?;
            }
            probes.keep(|| format!("stage{i}.out"), &x);
        }
        let x = self.post.forward(&leaky_relu(&x, 0.01)?)?;
        probes.keep(|| "post".into(), &x);
        let bins = self.stft.bins();
        // Speech stays far below the ceiling; bad weights can't reach infinity.
        let spec = x.narrow(1, 0, bins)?.minimum(LOG_CEILING)?.exp()?;
        let phase = x.narrow(1, bins, bins)?.sin()?;
        let audio = self.stft.inverse(&spec, &phase)?;
        probes.lap(|| "istft".into());
        Ok(audio)
    }
}

/// Channels of the text features carried into each decoder block.
const ASR: usize = 64;

/// Largest log-magnitude the generator may pass to `exp`, about 1e13.
const LOG_CEILING: f32 = 30.0;

pub(crate) struct Decoder {
    encode: AdainResBlk1d,
    decode: Vec<AdainResBlk1d>,
    f0_conv: Conv1d,
    noise_conv: Conv1d,
    asr_res: Conv1d,
    generator: Generator,
}

impl Decoder {
    /// `widths` are the text features' and the decoder's own.
    pub(crate) fn load(
        widths: (usize, usize),
        style: usize,
        c: &Istft,
        vb: &VarBuilder,
    ) -> Result<Self> {
        let (hidden, width) = widths;
        let wide = width + 2 + ASR;
        let decode = (0..4)
            .map(|i| {
                let out = if i == 3 {
                    c.upsample_initial_channel
                } else {
                    width
                };
                AdainResBlk1d::load((wide, out), style, i == 3, &vb.pp("decode").pp(i))
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            encode: AdainResBlk1d::load((hidden + 2, width), style, false, &vb.pp("encode"))?,
            decode,
            f0_conv: Conv::new(1, 1, 3)
                .stride(2)
                .padding(1)
                .load(&vb.pp("F0_conv"))?,
            noise_conv: Conv::new(1, 1, 3)
                .stride(2)
                .padding(1)
                .load(&vb.pp("N_conv"))?,
            asr_res: Conv::new(hidden, ASR, 1).load(&vb.pp("asr_res.0"))?,
            generator: Generator::load(c, style, &vb.pp("generator"))?,
        })
    }

    /// Aligned features, pitch and energy curves, and style, to samples.
    pub(crate) fn forward(
        &self,
        asr: &Tensor,
        curves: (&Tensor, &Tensor),
        s: &Tensor,
        noise: &mut Noise,
        probes: &mut Probes,
    ) -> Result<Vec<f32>> {
        let (f0_curve, energy) = curves;
        let f0 = self.f0_conv.forward(&f0_curve.unsqueeze(1)?)?;
        let n = self.noise_conv.forward(&energy.unsqueeze(1)?)?;
        let mut x = self.encode.forward(&Tensor::cat(&[asr, &f0, &n], 1)?, s)?;
        let res = self.asr_res.forward(asr)?;
        probes.lap(|| "decoder: encode".into());
        let mut residual = true;
        for block in &self.decode {
            if residual {
                x = Tensor::cat(&[&x, &res, &f0, &n], 1)?;
            }
            x = block.forward(&x, s)?;
            residual &= !block.upsamples();
            probes.lap(|| "decoder: blocks".into());
        }
        self.generator.forward(&x, s, f0_curve, noise, probes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampling_keeps_ends_and_halves() {
        let resample = |input: &[f32], len: usize| -> Vec<f32> {
            let at = |i: usize| input.get(i).copied().unwrap_or(0.0);
            resample_iter(input.len(), len, at).collect()
        };
        assert_eq!(resample(&[0.0, 2.0], 2), [0.0, 2.0]);
        assert_eq!(resample(&[1.0], 3), [1.0, 1.0, 1.0]);
        assert_eq!(resample(&[0.0, 1.0, 2.0, 3.0], 2), [0.5, 2.5]);
        assert!(resample(&[], 4).is_empty());
    }

    #[test]
    fn a_negative_level_has_phase_plus_pi_at_dc() -> Result<()> {
        let t = Tensor::from_vec(vec![-0.3f32; 100], (1, 100), &Device::Cpu)?;
        let (_, phase) = Stft::new(20, 5)?.transform(&t)?;
        let dc = phase.get(0)?.get(0)?.to_vec1::<f32>()?;
        let plus_pi = |p: &f32| (p - std::f32::consts::PI).abs() < 1e-6;
        assert!(dc.iter().all(plus_pi), "{dc:?}");
        Ok(())
    }

    #[test]
    fn the_inverse_stft_gives_back_what_it_was_given() -> Result<()> {
        let stft = Stft::new(20, 5)?;
        let wave: Vec<f32> = (0..400).map(|i| (i as f32 * 0.07).sin() * 0.5).collect();
        let t = Tensor::from_vec(wave.clone(), (1, 400), &Device::Cpu)?;
        let (m, p) = stft.transform(&t)?;
        let back = stft.inverse(&m, &p)?;
        assert_eq!(back.len(), wave.len());
        let worst = back
            .iter()
            .zip(&wave)
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(worst < 1e-4, "round trip off by {worst}");
        Ok(())
    }

    #[test]
    fn the_exported_stft_repeats_edges_and_sums_frames_unnormalised() -> Result<()> {
        let plain = Stft::new(20, 5)?;
        let rows = |table: &[f32], sign: f32, scale: f32| -> Vec<f32> {
            let windowed = table.as_chunks::<20>().0.iter().take(11).flat_map(|row| {
                row.iter()
                    .zip(&plain.window)
                    .map(move |(v, w)| sign * v * w / scale)
            });
            windowed.collect()
        };
        let kernels = Kernels {
            forward_real: rows(&plain.cos, 1.0, 1.0),
            forward_imag: rows(&plain.sin, -1.0, 1.0),
            backward_real: rows(&plain.cos, 1.0, 20.0),
            backward_imag: rows(&plain.sin, 1.0, 20.0),
        };
        let stft = Stft::new(20, 5)?.with_kernels(kernels);
        assert_eq!(stft.reflected(&[1.0; 11])?.len(), 31);
        let edges = stft.reflected(&(0..11).map(|i| i as f32).collect::<Vec<_>>())?;
        assert_eq!(
            (edges[0], edges[9], edges[10], edges[30]),
            (0.0, 0.0, 0.0, 10.0)
        );
        let silence = Tensor::zeros((1, 100), DType::F32, &Device::Cpu)?;
        let (m, _) = stft.transform(&silence)?;
        let floor = EXPORT_FLOOR.sqrt();
        assert!(
            m.flatten_all()?
                .to_vec1::<f32>()?
                .iter()
                .all(|v| *v == floor)
        );
        // DC alone: four Hann frames overlap to 2, over 20 points.
        let frames = 9;
        let mut mags = vec![0f32; 11 * frames];
        mags[..frames].fill(1.0);
        let m = Tensor::from_vec(mags, (1, 11, frames), &Device::Cpu)?;
        let back = stft.inverse(&m, &m.zeros_like()?)?;
        assert!((back[20] - 0.1).abs() < 1e-5, "{}", back[20]);
        Ok(())
    }

    #[test]
    fn the_exported_phase_is_atan2_with_pi_for_negative_reals() {
        use std::f32::consts::PI;
        for (re, im) in [
            (1.0, 1.0),
            (-1.0, 1.0),
            (-1.0, -1.0),
            (0.0, 1.0),
            (0.3, -2.0),
        ] {
            assert!((export_phase(re, im) - f32::atan2(im, re)).abs() < 1e-6);
        }
        assert_eq!(export_phase(-1.0, -0.0), PI);
        assert_eq!(export_phase(-1.0, 0.0), PI);
        assert_eq!(export_phase(0.0, 0.0), 0.0);
    }
}
