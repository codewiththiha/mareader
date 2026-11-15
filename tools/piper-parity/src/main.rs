//! Checks mareader-vits on Piper voices against onnxruntime and espeak.

//! Usage: prepare <voices> <g2p> <out>; reference.py <voices> <out>;
//! then compare <voices> <out>.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::time::Instant;

use mareader_g2p::{Dialect, G2p};
use mareader_vits::{Config, Scales, Vits, espeak_like};
use parity_kit::{correlation, edits, read_f32, write_wav};
use serde::{Deserialize, Serialize};

type R<T> = Result<T, Box<dyn std::error::Error>>;

const SENTENCES: [&str; 10] = [
    "Hello world.",
    "The quick brown fox jumps over the lazy dog, then naps in the warm afternoon sun.",
    "Reading aloud should feel natural: steady pacing, clear words, and pauses where \
     a person would breathe.",
    "Does it sound right to you?",
    "I hope it does, because this sentence is long enough to need a fair number of \
     phonemes, and the voice must keep its rhythm all the way through to the very end.",
    "On the third of May, nineteen ninety-nine, she paid forty-two dollars for a ticket.",
    "\"Wait!\" he shouted. \"Don't open that door.\"",
    "Thoroughly thoughtful thinkers rarely rush their reasoning.",
    "Our neighbour's colour television was already there when the water heater broke.",
    "Yes.",
];

/// Waveform correlation each noiseless sentence must reach.
const SAME_WAVE: f64 = 0.999;
/// Share of noiseless sentences whose length must match to the sample.
const SAME_LENGTH: f64 = 0.8;
/// How far total length and loudness may drift with noise on.
const LENGTH_DRIFT: f64 = 0.15;
const LOUDNESS_DRIFT: f64 = 0.25;
/// Phoneme error against espeak each voice must stay under.
const PHONEME_ERROR: f64 = 0.25;

#[derive(Serialize, Deserialize)]
struct Case {
    voice: String,
    index: usize,
    text: String,
    misaki: String,
    phonemes: String,
    ids: Vec<u32>,
    speaker: usize,
}

#[derive(Deserialize)]
struct Timing {
    seconds: f64,
    samples: u64,
}

#[derive(Deserialize)]
struct Reference {
    timing: HashMap<String, Timing>,
    espeak: HashMap<String, HashMap<String, String>>,
}

fn voices(dir: &Path) -> R<Vec<(String, Config)>> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if let Some(voice) = name.strip_suffix(".onnx.json") {
            out.push((voice.to_owned(), Config::parse(&std::fs::read(&path)?)?));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

fn prepare(voices_dir: &Path, g2p_dir: &Path, out: &Path) -> R<()> {
    let mut g2ps: HashMap<Dialect, G2p> = HashMap::new();
    let mut cases = Vec::new();
    for (voice, config) in voices(voices_dir)? {
        let style = config.style().ok_or(format!("{voice}: no phoneme style"))?;
        let dialect = if style.british() {
            Dialect::British
        } else {
            Dialect::American
        };
        let g2p = match g2ps.entry(dialect) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => e.insert(G2p::load(g2p_dir, dialect)?),
        };
        for (index, text) in SENTENCES.iter().enumerate() {
            let misaki = g2p.phonemize(text);
            let phonemes = espeak_like(&misaki, style);
            let ids = config.ids(&phonemes)?;
            let speakers = config.num_speakers.max(1) as usize;
            cases.push(Case {
                voice: voice.clone(),
                index,
                text: (*text).to_owned(),
                misaki,
                phonemes,
                ids,
                speaker: (index * 7) % speakers,
            });
        }
    }
    std::fs::create_dir_all(out)?;
    std::fs::write(out.join("cases.json"), serde_json::to_vec_pretty(&cases)?)?;
    println!("prepared {} sentences", cases.len());
    Ok(())
}

fn energy(v: &[f32]) -> f64 {
    v.iter().map(|&x| f64::from(x) * f64::from(x)).sum()
}

/// Each voice's milliseconds per step, steps in the order first met.
#[derive(Default)]
struct Laps {
    voices: Vec<String>,
    steps: Vec<(String, Vec<f64>)>,
}

impl Laps {
    fn add(&mut self, voice: &str, steps: &[(String, f64)]) {
        let col = match self.voices.iter().position(|v| v == voice) {
            Some(c) => c,
            None => {
                self.voices.push(voice.to_owned());
                self.voices.len() - 1
            }
        };
        for (name, ms) in steps {
            let at = match self.steps.iter().position(|(n, _)| n == name) {
                Some(at) => at,
                None => {
                    self.steps.push((name.clone(), Vec::new()));
                    self.steps.len() - 1
                }
            };
            if let Some((_, row)) = self.steps.get_mut(at) {
                row.resize(row.len().max(col + 1), 0.0);
                if let Some(cell) = row.get_mut(col) {
                    *cell += ms;
                }
            }
        }
    }

    fn table(&self) -> String {
        let mut t = format!("| Step (ms) | {} |\n|---|", self.voices.join(" | "));
        t.push_str(&"---|".repeat(self.voices.len()));
        t.push('\n');
        let mut totals = vec![0.0; self.voices.len()];
        for (name, row) in &self.steps {
            let _ = write!(t, "| {name} |");
            for (c, total) in totals.iter_mut().enumerate() {
                let ms = row.get(c).copied().unwrap_or(0.0);
                *total += ms;
                let _ = write!(t, " {ms:.0} |");
            }
            t.push('\n');
        }
        t.push_str("| total |");
        for ms in totals {
            let _ = write!(t, " {ms:.0} |");
        }
        t.push('\n');
        t
    }
}

#[derive(Default)]
struct Tally {
    sentences: usize,
    same_length: usize,
    worst_corr: f64,
    worst_diff: f32,
    off_by_frames: usize,
    noisy_samples: usize,
    spoken_samples: usize,
    ref_samples: usize,
    ours_energy: f64,
    ref_energy: f64,
    ours_seconds: f64,
    edits: usize,
    espeak_len: usize,
    exact_phonemes: usize,
}

fn compare(voices_dir: &Path, out: &Path) -> R<bool> {
    let cases: Vec<Case> = serde_json::from_slice(&std::fs::read(out.join("cases.json"))?)?;
    let reference: Reference =
        serde_json::from_slice(&std::fs::read(out.join("ref").join("reference.json"))?)?;
    let wav_dir = out.join("wav");
    std::fs::create_dir_all(&wav_dir)?;
    let mut md = String::from("# Piper voices: mareader-vits against onnxruntime\n\n");
    let mut all_ok = true;
    let mut rows = String::new();
    let mut speed = String::new();
    let mut said = String::new();
    let mut laps = Laps::default();
    for (voice, config) in voices(voices_dir)? {
        let path = voices_dir.join(format!("{voice}.onnx"));
        let start = Instant::now();
        let vits = Vits::load(&path)?;
        let load = start.elapsed().as_secs_f64();
        let inf = config.inference;
        let quiet = Scales {
            noise: 0.0,
            noise_w: 0.0,
            ..inf
        };
        let mut t = Tally {
            worst_corr: 1.0,
            ..Tally::default()
        };
        for case in cases.iter().filter(|c| c.voice == voice) {
            let stem = format!("{voice}.{}", case.index);
            let ref_det = read_f32(&out.join("ref").join(format!("{stem}.det.f32")))?;
            let ref_noisy = read_f32(&out.join("ref").join(format!("{stem}.noisy.f32")))?;
            let start = Instant::now();
            let (det, steps) = vits.speak_timed(&case.ids, case.speaker, quiet)?;
            laps.add(&voice, &steps.steps);
            let noisy = vits.speak(&case.ids, case.speaker, inf)?;
            t.ours_seconds += start.elapsed().as_secs_f64();
            t.sentences += 1;
            if det.len() == ref_det.len() {
                t.same_length += 1;
                t.worst_corr = t.worst_corr.min(correlation(&det, &ref_det));
                let diff = det
                    .iter()
                    .zip(&ref_det)
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0f32, f32::max);
                t.worst_diff = t.worst_diff.max(diff);
            } else {
                t.off_by_frames += det.len().abs_diff(ref_det.len()) / 256;
            }
            t.noisy_samples += noisy.len();
            t.spoken_samples += noisy.len() + det.len();
            t.ref_samples += ref_noisy.len();
            t.ours_energy += energy(&noisy);
            t.ref_energy += energy(&ref_noisy);
            let espeak = reference
                .espeak
                .get(&voice)
                .and_then(|m| m.get(&case.index.to_string()))
                .cloned()
                .unwrap_or_default();
            let ours = case.phonemes.trim().to_owned();
            let e = edits(&ours, espeak.trim());
            t.edits += e;
            t.espeak_len += espeak.trim().chars().count();
            t.exact_phonemes += usize::from(e == 0);
            if case.index < 3 {
                let rate = config.audio.sample_rate;
                write_wav(&wav_dir.join(format!("{stem}.ours.wav")), &noisy, rate)?;
                write_wav(&wav_dir.join(format!("{stem}.ref.wav")), &ref_noisy, rate)?;
            }
            if case.index < 2 {
                let _ = writeln!(
                    said,
                    "| {voice} | {} | `{}` | `{}` | `{}` |",
                    case.text, case.misaki, ours, espeak
                );
            }
        }
        let length_ratio = t.noisy_samples as f64 / t.ref_samples.max(1) as f64;
        let loud_ratio = (t.ours_energy / t.noisy_samples.max(1) as f64).sqrt()
            / (t.ref_energy / t.ref_samples.max(1) as f64)
                .sqrt()
                .max(1e-12);
        let same_share = t.same_length as f64 / t.sentences.max(1) as f64;
        let per = t.edits as f64 / t.espeak_len.max(1) as f64;
        let ok = t.sentences > 0
            && same_share >= SAME_LENGTH
            && t.worst_corr >= SAME_WAVE
            && (length_ratio - 1.0).abs() <= LENGTH_DRIFT
            && (loud_ratio - 1.0).abs() <= LOUDNESS_DRIFT
            && per <= PHONEME_ERROR;
        all_ok &= ok;
        let _ = writeln!(
            rows,
            "| {voice} | {} Hz, {} speaker(s) | {}/{} | {:.6} | {:.1e} | {:.3} | {:.3} | {:.1}% ({} exact) | {} |",
            config.audio.sample_rate,
            vits.speakers(),
            t.same_length,
            t.sentences,
            t.worst_corr,
            t.worst_diff,
            length_ratio,
            loud_ratio,
            per * 100.0,
            t.exact_phonemes,
            if ok { "yes" } else { "**no**" }
        );
        let audio_s = t.spoken_samples as f64 / f64::from(config.audio.sample_rate);
        let (ref_s, ref_audio) = reference
            .timing
            .get(&voice)
            .map(|r| {
                (
                    r.seconds,
                    r.samples as f64 / f64::from(config.audio.sample_rate),
                )
            })
            .unwrap_or((0.0, 0.0));
        let _ = writeln!(
            speed,
            "| {voice} | {load:.2} s | {:.3} | {:.3} |",
            t.ours_seconds / audio_s.max(1e-9),
            ref_s / ref_audio.max(1e-9),
        );
    }
    let _ = writeln!(
        md,
        "Noiseless (scales 0, length, 0): sample-exact length and waveform correlation.\n\
         With noise on: total length and RMS against onnxruntime's (its noise differs).\n\
         Phonemes: our G2P respelled, against espeak's for the same text.\n\n\
         | Voice | Audio | Same length | Worst corr | Max diff | Length (noise) | RMS (noise) | Phoneme error | Pass |\n\
         |---|---|---|---|---|---|---|---|---|\n{rows}\n\
         Gates: corr >= {SAME_WAVE}, same length >= {:.0}%, length within {:.0}%, \
         RMS within {:.0}%, phoneme error <= {:.0}%.\n\n\
         ## Speed (seconds of compute per second of speech)\n\n\
         | Voice | Load | Ours | onnxruntime |\n|---|---|---|---|\n{speed}\n\
         ## Where the time goes (ms over the noiseless runs)\n\n{}\n\
         ## What was said\n\n\
         | Voice | Text | misaki | Ours | espeak |\n|---|---|---|---|---|\n{said}",
        SAME_LENGTH * 100.0,
        LENGTH_DRIFT * 100.0,
        LOUDNESS_DRIFT * 100.0,
        PHONEME_ERROR * 100.0,
        laps.table(),
    );
    md.push_str(if all_ok {
        "\n**All checks pass.**\n"
    } else {
        "\n**Some checks fail.**\n"
    });
    std::fs::write(out.join("summary.md"), &md)?;
    println!("{md}");
    Ok(all_ok)
}

fn main() -> R<()> {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize| -> R<PathBuf> {
        Ok(PathBuf::from(
            args.get(i).ok_or("see the usage at the top of main.rs")?,
        ))
    };
    match args.get(1).map(String::as_str) {
        Some("prepare") => prepare(&arg(2)?, &arg(3)?, &arg(4)?),
        Some("compare") => {
            if compare(&arg(2)?, &arg(3)?)? {
                Ok(())
            } else {
                Err("some checks fail".into())
            }
        }
        _ => Err("usage: piper-parity prepare|compare …".into()),
    }
}
