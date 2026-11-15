//! Runs mareader-kokoro and the reference on one model, stage by stage.

//! Usage: kokoro-parity <model dir> <out dir> [g2p | builds]

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use any_tts::models::kokoro::albert::Albert;
use any_tts::models::kokoro::config::KokoroConfig;
use any_tts::models::kokoro::decoder::IstftDecoder;
use any_tts::models::kokoro::phonemizer;
use any_tts::models::kokoro::prosody::ProsodyPredictor;
use any_tts::models::kokoro::text_encoder::TextEncoder;
use any_tts::{ModelType, SynthesisRequest, TtsConfig};
use candle_core::{DType, Device, Module, Tensor};
use candle_nn::VarBuilder;
use mareader_kokoro as ours;
use parity_kit::write_wav;

mod builds;
mod g2p;
mod own;
mod refdecoder;

type R<T> = Result<T, Box<dyn std::error::Error>>;

const SENTENCES: [&str; 3] = [
    "Hello world.",
    "The quick brown fox jumps over the lazy dog, then naps in the warm afternoon sun.",
    "Reading aloud should feel natural: steady pacing, clear words, and pauses where \
     a person would breathe. Does it sound right to you? I hope it does, because \
     this sentence is long enough to need a fair number of phonemes.",
];

/// Noise seed both decoders draw from when compared step by step.
const TWIN_SEED: u64 = 0x5EED;

/// Waveform correlation our audio must pass against the twin, noise equal.
const TWIN_AGREES: f64 = 0.99;

/// Relative error under which a stage counts as the same.
const EXACT: f32 = 1e-3;
/// Relative error under which a stage counts as close enough.
const CLOSE: f32 = 1e-2;

struct Reference {
    bert: Albert,
    bert_encoder: candle_nn::Linear,
    predictor: ProsodyPredictor,
    text: TextEncoder,
    decoder: IstftDecoder,
    /// The same decoder's code, vendored, noiseless and probed.
    twin: refdecoder::IstftDecoder,
}

/// The published `.pth`'s weights, by the names the reference reads.
fn published(weights: &Path) -> R<HashMap<String, Tensor>> {
    let mut all = HashMap::new();
    for part in [
        "bert",
        "bert_encoder",
        "decoder",
        "predictor",
        "text_encoder",
    ] {
        for (name, t) in candle_core::pickle::read_all_with_key(weights, Some(part))? {
            let name = format!("{part}.{name}").replace(".module.", ".");
            all.insert(name, t.to_dtype(DType::F32)?);
        }
    }
    Ok(all)
}

impl Reference {
    fn load(c: &KokoroConfig, weights: &Path) -> R<Self> {
        Self::from_tensors(c, published(weights)?)
    }

    fn from_tensors(c: &KokoroConfig, all: HashMap<String, Tensor>) -> R<Self> {
        let cpu = Device::Cpu;
        let vb = VarBuilder::from_tensors(all, DType::F32, &cpu);
        let i = &c.istftnet;
        let twin = refdecoder::IstftDecoder::load(
            c.hidden_dim,
            c.style_dim,
            c.n_mels,
            &i.resblock_kernel_sizes,
            &i.upsample_rates,
            i.upsample_initial_channel,
            &i.resblock_dilation_sizes,
            &i.upsample_kernel_sizes,
            i.gen_istft_n_fft,
            i.gen_istft_hop_size,
            vb.pp("decoder"),
            &cpu,
            DType::F32,
        )?;
        Ok(Self {
            twin,
            bert: Albert::load(&c.plbert, vb.pp("bert"), &cpu)?,
            bert_encoder: candle_nn::linear(
                c.plbert.hidden_size,
                c.hidden_dim,
                vb.pp("bert_encoder"),
            )?,
            predictor: ProsodyPredictor::load(
                c.style_dim,
                c.hidden_dim,
                c.n_layer,
                c.max_dur,
                vb.pp("predictor"),
                &cpu,
            )?,
            text: TextEncoder::load(
                c.hidden_dim,
                c.text_encoder_kernel_size,
                c.n_layer,
                c.n_token,
                vb.pp("text_encoder"),
                &cpu,
            )?,
            decoder: IstftDecoder::load(
                c.hidden_dim,
                c.style_dim,
                c.n_mels,
                &i.resblock_kernel_sizes,
                &i.upsample_rates,
                i.upsample_initial_channel,
                &i.resblock_dilation_sizes,
                &i.upsample_kernel_sizes,
                i.gen_istft_n_fft,
                i.gen_istft_hop_size,
                vb.pp("decoder"),
                &cpu,
                DType::F32,
            )?,
        })
    }
}

/// The reference's outputs at each stage, all flattened.
struct Stages {
    bert: Vec<f32>,
    projected: Vec<f32>,
    encoded: Vec<f32>,
    raw_durations: Vec<f32>,
    pitch: Vec<f32>,
    energy: Vec<f32>,
    text: Vec<f32>,
    audio: Vec<f32>,
    took: Duration,
    /// The vendored twin's noiseless audio and generator steps.
    twin_audio: Vec<f32>,
    twin_probes: Vec<(String, Vec<f32>)>,
}

fn flat(t: &Tensor) -> R<Vec<f32>> {
    Ok(t.flatten_all()?.to_dtype(DType::F32)?.to_vec1()?)
}

/// Runs the reference, aligned by `durations` so frame counts agree.
fn reference(r: &Reference, ids: &[u32], style: &[f32], durations: &[usize]) -> R<Stages> {
    let cpu = Device::Cpu;
    let start = Instant::now();
    let style = Tensor::from_vec(style.to_vec(), (1, style.len()), &cpu)?;
    let ids_t = Tensor::new(ids, &cpu)?.unsqueeze(0)?;
    let lengths = Tensor::new(&[ids.len() as u32], &cpu)?;
    let mask = Tensor::zeros((1, ids.len()), DType::U8, &cpu)?;
    let half = style.dim(1)? / 2;
    let s_pred = style.narrow(1, half, half)?;
    let s_dec = style.narrow(1, 0, half)?;
    let bert = r.bert.forward_unmasked(&ids_t)?;
    let projected = r.bert_encoder.forward(&bert)?.transpose(1, 2)?;
    let encoded = r
        .predictor
        .text_encoder
        .forward(&projected, &s_pred, &lengths, &mask)?;
    let raw = r.predictor.predict_duration(&encoded, &s_pred)?;
    let picks: Vec<u32> = durations
        .iter()
        .enumerate()
        .flat_map(|(i, &d)| std::iter::repeat_n(i as u32, d))
        .collect();
    let frames = picks.len();
    let picks = Tensor::from_vec(picks, frames, &cpu)?;
    let aligned = encoded
        .transpose(1, 2)?
        .contiguous()?
        .index_select(&picks, 2)?;
    let (pitch, energy) = r.predictor.f0_n_predict(&aligned, &s_pred)?;
    let text = r.text.forward(&ids_t, &lengths, &mask)?;
    let asr = text.contiguous()?.index_select(&picks, 2)?;
    let audio = r.decoder.forward(&asr, &pitch, &energy, &s_dec)?;
    let took = start.elapsed();
    // The twin draws noise exactly as ours does, from the same seed.
    refdecoder::SEED.with(|s| s.set(Some(TWIN_SEED)));
    refdecoder::PROBES.with(|p| p.borrow_mut().clear());
    let twin_audio = flat(&r.twin.forward(&asr, &pitch, &energy, &s_dec)?)?;
    let twin_probes = refdecoder::PROBES.with(|p| std::mem::take(&mut *p.borrow_mut()));
    refdecoder::SEED.with(|s| s.set(None));
    Ok(Stages {
        twin_audio,
        twin_probes,
        bert: flat(&bert)?,
        projected: flat(&projected)?,
        encoded: flat(&encoded)?,
        raw_durations: flat(&raw)?,
        pitch: flat(&pitch)?,
        energy: flat(&energy)?,
        text: flat(&text)?,
        audio: flat(&audio)?,
        took,
    })
}

/// Each stage of `trace` against the reference's, as table rows.
fn compare_stages(
    tag: &str,
    trace: &ours::Trace,
    theirs: &Stages,
    (stage_rows, failed): (&mut String, &mut Vec<String>),
) -> R<()> {
    let stages: [(&str, Vec<f32>, &[f32]); 7] = [
        (
            "phoneme encoder",
            trace.bert.flatten_all()?.to_vec1()?,
            &theirs.bert,
        ),
        (
            "projection",
            trace.projected.flatten_all()?.to_vec1()?,
            &theirs.projected,
        ),
        (
            "duration encoder",
            trace.encoded.flatten_all()?.to_vec1()?,
            &theirs.encoded,
        ),
        (
            "durations (raw)",
            trace.raw_durations.flatten_all()?.to_vec1()?,
            &theirs.raw_durations,
        ),
        (
            "pitch curve",
            trace.pitch.flatten_all()?.to_vec1()?,
            &theirs.pitch,
        ),
        (
            "energy curve",
            trace.energy.flatten_all()?.to_vec1()?,
            &theirs.energy,
        ),
        (
            "text encoder",
            trace.text.flatten_all()?.to_vec1()?,
            &theirs.text,
        ),
    ];
    for (name, mine, theirs) in &stages {
        let (verdict, worst, rel) = match diff(mine, theirs) {
            Some((w, r)) if r < EXACT => ("same", w, r),
            Some((w, r)) if r < CLOSE => ("close", w, r),
            Some((w, r)) => ("**DIFFERS**", w, r),
            None => ("**SHAPE**", f32::NAN, f32::NAN),
        };
        if verdict.starts_with("**") {
            failed.push(format!("{tag} {name}: {verdict} (rel {rel:e})"));
        }
        writeln!(
            stage_rows,
            "| {tag} | {name} | {worst:.2e} | {rel:.2e} | {verdict} |"
        )?;
    }
    let theirs_rounded: Vec<usize> = theirs
        .raw_durations
        .iter()
        .map(|d| (d + 2.5e-4).round().max(1.0) as usize)
        .collect();
    let off = trace
        .durations
        .iter()
        .zip(&theirs_rounded)
        .filter(|(a, b)| a != b)
        .count();
    writeln!(
        stage_rows,
        "| {tag} | durations (frames) | {off} of {} differ | | {} |",
        trace.durations.len(),
        if off == 0 { "same" } else { "**DIFFERS**" }
    )?;
    if off > 0 {
        failed.push(format!("{tag}: {off} rounded durations differ"));
    }
    Ok(())
}

/// Largest absolute difference, and that over the reference's largest value.
fn diff(a: &[f32], b: &[f32]) -> Option<(f32, f32)> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    // A NaN anywhere must not pass as no difference.
    let worst = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0f32, |m, d| {
            if m.is_nan() || d.is_nan() {
                f32::NAN
            } else {
                m.max(d)
            }
        });
    let scale = b.iter().map(|v| v.abs()).fold(0f32, f32::max).max(1e-12);
    Some((worst, worst / scale))
}

fn pearson(a: &[f32], b: &[f32]) -> f64 {
    let n = a.len().min(b.len());
    if n < 2 {
        return 0.0;
    }
    let (a, b) = (&a[..n], &b[..n]);
    let ma = a.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let mb = b.iter().map(|&v| f64::from(v)).sum::<f64>() / n as f64;
    let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
    for (&x, &y) in a.iter().zip(b) {
        let (x, y) = (f64::from(x) - ma, f64::from(y) - mb);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    ab / (aa.sqrt() * bb.sqrt()).max(1e-30)
}

fn rms(a: &[f32]) -> f64 {
    (a.iter().map(|&v| f64::from(v).powi(2)).sum::<f64>() / a.len().max(1) as f64).sqrt()
}

/// Loudness per 10 ms, in decibels.
fn envelope(a: &[f32]) -> Vec<f32> {
    a.chunks(240)
        .map(|c| (20.0 * rms(c).max(1e-5).log10()) as f32)
        .collect()
}

/// Agreement of two takes of the same speech.
struct Audio {
    lengths: (usize, usize),
    rms: (f64, f64),
    envelope: f64,
    wave: f64,
}

impl Audio {
    fn of(ours: &[f32], theirs: &[f32]) -> Self {
        Self {
            lengths: (ours.len(), theirs.len()),
            rms: (rms(ours), rms(theirs)),
            envelope: pearson(&envelope(ours), &envelope(theirs)),
            wave: pearson(ours, theirs),
        }
    }

    /// Two reference runs, noise apart, agree at about 0.994.
    fn passes(&self) -> bool {
        let ratio = self.rms.0 / self.rms.1.max(1e-12);
        let near = self.lengths.0.abs_diff(self.lengths.1) <= self.lengths.1 / 50;
        near && (0.9..=1.1).contains(&ratio) && self.envelope > 0.98 && self.wave > 0.98
    }

    fn row(&self, what: &str) -> String {
        format!(
            "| {what} | {} / {} | {:.4} / {:.4} | {:.4} | {:.4} | {} |",
            self.lengths.0,
            self.lengths.1,
            self.rms.0,
            self.rms.1,
            self.envelope,
            self.wave,
            if self.passes() { "ok" } else { "**DIFFERS**" },
        )
    }
}

fn seconds(samples: usize) -> f64 {
    samples as f64 / f64::from(ours::SAMPLE_RATE)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    let (Some(dir), Some(out)) = (args.get(1), args.get(2)) else {
        return Err("usage: kokoro-parity <model dir> <out dir> [g2p | builds]".into());
    };
    let (dir, out) = (PathBuf::from(dir), PathBuf::from(out));
    std::fs::create_dir_all(&out)?;

    let mut report = String::new();
    let mut failed = Vec::new();
    g2p::check(&dir, &mut report, &mut failed)?;
    if args.get(3).is_some_and(|a| a == "g2p") {
        return finish(&out, &report, &failed);
    }
    if args.get(3).is_some_and(|a| a == "builds") {
        let start = Instant::now();
        let model = ours::Kokoro::load(&dir)?;
        let took = start.elapsed().as_secs_f64();
        writeln!(report, "Loading Full: {took:.2} s.\n")?;
        let voice = ours::Voice::load(&dir.join("voices/af_heart.pt"))?;
        let config = KokoroConfig::from_bytes(std::fs::read(dir.join("config.json"))?)?;
        let mut inputs = Vec::new();
        for (n, text) in SENTENCES.iter().enumerate() {
            let phonemes = phonemizer::phonemize(text, "en-us", &config.vocab)?;
            let chunks = model.chunks(&model.normalise(&phonemes));
            let first = chunks.first().ok_or("no chunks")?;
            let ids = [&[0u32][..], &first.ids, &[0]].concat();
            inputs.push((format!("s{}", n + 1), ids));
        }
        let pair = (&model, &voice);
        builds::check(&dir, &out, pair, &inputs, (&mut report, &mut failed))?;
        drop(model);
        own::check(&dir, &config, &voice, &inputs, (&mut report, &mut failed))?;
        return finish(&out, &report, &failed);
    }

    let start = Instant::now();
    let model = like_reference(&dir)?;
    let our_load = start.elapsed();
    let voice = ours::Voice::load(&dir.join("voices/af_heart.pt"))?;
    let config = KokoroConfig::from_bytes(std::fs::read(dir.join("config.json"))?)?;
    let start = Instant::now();
    let reference_model = Reference::load(&config, &dir.join(ours::WEIGHTS))?;
    let ref_load = start.elapsed();
    let full = any_tts::load_model(
        TtsConfig::new(ModelType::Kokoro).with_model_path(dir.to_string_lossy().to_string()),
    )?;

    writeln!(report, "## Kokoro parity\n")?;
    writeln!(
        report,
        "Loading: ours {:.2} s, reference {:.2} s.\n",
        our_load.as_secs_f64(),
        ref_load.as_secs_f64()
    )?;
    let mut stage_rows = String::new();
    let mut audio_rows = String::new();
    let mut speed_rows = String::new();
    let mut diag_rows = String::new();
    let mut twin_rows = String::new();
    let mut inputs = Vec::new();

    for (n, text) in SENTENCES.iter().enumerate() {
        let tag = format!("s{}", n + 1);
        let phonemes = phonemizer::phonemize(text, "en-us", &config.vocab)?;
        let normal = model.normalise(&phonemes);
        if normal != phonemes {
            failed.push(format!(
                "{tag}: normalising changed {phonemes:?} to {normal:?}"
            ));
        }
        let chunks = model.chunks(&normal);
        let Some(first) = chunks.first() else {
            return Err(format!("{tag}: no chunks").into());
        };
        let mut ids = vec![0u32];
        ids.extend_from_slice(&first.ids);
        ids.push(0);
        let style = voice.style(ids.len())?;
        let style_v: Vec<f32> = style.flatten_all()?.to_vec1()?;
        inputs.push((tag.clone(), ids.clone()));

        let start = Instant::now();
        let trace = model.trace(&ids, &style, 1.0)?;
        let our_took = start.elapsed();
        let theirs = reference(&reference_model, &ids, &style_v, &trace.durations)?;

        compare_stages(&tag, &trace, &theirs, (&mut stage_rows, &mut failed))?;

        let audio = Audio::of(&trace.audio, &theirs.audio);
        if !audio.passes() {
            failed.push(format!("{tag}: decoded audio differs"));
        }
        writeln!(audio_rows, "{}", audio.row(&format!("{tag} decoder")))?;
        write_wav(&out.join(format!("{tag}-ours.wav")), &trace.audio, ours::SAMPLE_RATE)?;
        // Which side the gap comes from: our noise, or something fixed.
        let reseeded = model
            .trace_noise(&ids, &style, 1.0, 12345, 1.0, false)?
            .audio;
        let quiet = model.trace_noise(&ids, &style, 1.0, 0, 0.0, false)?.audio;
        let twin_trace = model.trace_noise(&ids, &style, 1.0, TWIN_SEED, 1.0, true)?;
        // Same noise, both sides: each generator step should agree.
        for (name, theirs_v) in &theirs.twin_probes {
            let mine = twin_trace.probes.iter().find(|(n, _)| n == name);
            let row = match mine {
                Some((_, t)) => {
                    let v: Vec<f32> = t.flatten_all()?.to_vec1()?;
                    match diff(&v, theirs_v) {
                        Some((w, r)) => format!("{w:.2e} | {r:.2e} | {:.5}", pearson(&v, theirs_v)),
                        None => format!("sizes {} / {} | | ", v.len(), theirs_v.len()),
                    }
                }
                None => "missing here | | ".into(),
            };
            writeln!(twin_rows, "| {tag} | {name} | {row} |")?;
        }
        let twin_corr = pearson(&twin_trace.audio, &theirs.twin_audio);
        if twin_corr.is_nan() || twin_corr <= TWIN_AGREES {
            failed.push(format!("{tag}: audio differs from the twin on equal noise"));
        }
        writeln!(twin_rows, "| {tag} | audio | | | {twin_corr:.5} |")?;
        writeln!(
            twin_rows,
            "| {tag} | twin vs published reference | | | {:.5} |",
            pearson(&theirs.twin_audio, &theirs.audio)
        )?;
        let again = reference(&reference_model, &ids, &style_v, &trace.durations)?.audio;
        writeln!(
            diag_rows,
            "| {tag} | {:.4} | {:.4} | {:.4} | {:.4} | {:.4} |",
            pearson(&again, &theirs.audio),
            pearson(&reseeded, &trace.audio),
            pearson(&quiet, &trace.audio),
            pearson(&quiet, &theirs.audio),
            rms(&quiet) / rms(&theirs.audio),
        )?;
        write_wav(&out.join(format!("{tag}-reference.wav")), &theirs.audio, ours::SAMPLE_RATE)?;

        // End to end: every chunk, voice rows and pauses included.
        let mut whole = Vec::new();
        let start = Instant::now();
        model.speak(&normal, &voice, 1.0, |chunk| {
            whole.extend(chunk);
            true
        })?;
        let our_whole = start.elapsed();
        let request = SynthesisRequest::new(*text)
            .with_voice("af_heart")
            .with_language("en-us");
        let start = Instant::now();
        let their_whole = full.synthesize(&request)?.samples;
        let ref_whole = start.elapsed();
        let audio = Audio::of(&whole, &their_whole);
        if !audio.passes() {
            failed.push(format!("{tag}: whole-text audio differs"));
        }
        writeln!(audio_rows, "{}", audio.row(&format!("{tag} whole text")))?;
        write_wav(&out.join(format!("{tag}-whole-ours.wav")), &whole, ours::SAMPLE_RATE)?;
        write_wav(
            &out.join(format!("{tag}-whole-reference.wav")),
            &their_whole,
            ours::SAMPLE_RATE,
        )?;

        let said = seconds(trace.audio.len());
        writeln!(
            speed_rows,
            "| {tag} | {} | {} | {said:.2} s | {:.2} s ({:.1}× real time) | {:.2} s ({:.1}×) | \
             {:.2} s / {:.2} s |",
            ids.len(),
            chunks.len(),
            our_took.as_secs_f64(),
            said / our_took.as_secs_f64(),
            theirs.took.as_secs_f64(),
            said / theirs.took.as_secs_f64(),
            our_whole.as_secs_f64(),
            ref_whole.as_secs_f64(),
        )?;
        println!("{tag}: {normal}");
    }

    writeln!(report, "### Stages (first chunk, same input)\n")?;
    writeln!(
        report,
        "| text | stage | max abs diff | relative | verdict |"
    )?;
    writeln!(report, "|---|---|---|---|---|")?;
    report.push_str(&stage_rows);
    writeln!(report, "\n### Audio (ours / reference)\n")?;
    writeln!(
        report,
        "| take | samples | RMS | loudness-curve corr | waveform corr | verdict |"
    )?;
    writeln!(report, "|---|---|---|---|---|---|")?;
    report.push_str(&audio_rows);
    writeln!(report, "\n### Noise diagnostics (waveform correlation)\n")?;
    writeln!(
        report,
        "| text | ref vs ref | ours vs ours reseeded | ours noiseless vs ours | \
         ours noiseless vs ref | noiseless RMS / ref |"
    )?;
    writeln!(report, "|---|---|---|---|---|---|")?;
    report.push_str(&diag_rows);
    writeln!(
        report,
        "\n### Generator steps, same noise, ours vs vendored reference\n"
    )?;
    writeln!(
        report,
        "| text | step | max abs diff | relative | correlation |"
    )?;
    writeln!(report, "|---|---|---|---|---|")?;
    report.push_str(&twin_rows);
    writeln!(report, "\n### Speed (CPU, release)\n")?;
    writeln!(
        report,
        "| text | tokens | chunks | audio | ours | reference | whole text ours / ref |"
    )?;
    writeln!(report, "|---|---|---|---|---|---|---|")?;
    report.push_str(&speed_rows);
    writeln!(report)?;
    drop(model);
    // The builds are compared as the app runs them.
    let app = ours::Kokoro::load(&dir)?;
    builds::check(
        &dir,
        &out,
        (&app, &voice),
        &inputs,
        (&mut report, &mut failed),
    )?;
    drop(app);
    own::check(&dir, &config, &voice, &inputs, (&mut report, &mut failed))?;
    finish(&out, &report, &failed)
}

/// Ours with the reference's exact GELU; the app keeps PyTorch's tanh form.
fn like_reference(dir: &Path) -> R<ours::Kokoro> {
    let mut config = ours::Config::parse(&std::fs::read(dir.join("config.json"))?)?;
    config.plbert.hidden_act = ours::Activation::Gelu;
    Ok(ours::Kokoro::from_tensors(
        config,
        ours::read_model(&dir.join(ours::WEIGHTS))?,
    )?)
}

/// Writes the summary and turns any failure into an error.
fn finish(out: &Path, report: &str, failed: &[String]) -> R<()> {
    let mut report = report.to_owned();
    if failed.is_empty() {
        writeln!(report, "**All checks pass.**")?;
    } else {
        writeln!(report, "**{} check(s) failed:**\n", failed.len())?;
        for f in failed {
            writeln!(report, "- {f}")?;
        }
    }
    std::fs::write(out.join("summary.md"), &report)?;
    println!("{report}");
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!("{} parity check(s) failed", failed.len()).into())
    }
}
