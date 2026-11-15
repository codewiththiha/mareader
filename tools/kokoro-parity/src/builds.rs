//! Each smaller build against the full weights, on the same phonemes.

use std::fmt::Write as _;
use std::path::Path;
use std::time::Instant;

use mareader_kokoro as ours;
use parity_kit::write_wav;

use crate::{R, envelope, pearson, rms, seconds};

/// The builds offered, by name, file and the agreement each must reach.
const BUILDS: [(&str, &str, Gate); 3] = [
    (
        "Half",
        "kokoro-82m-f16.gguf",
        Gate {
            envelope: 0.98,
            length: 0.02,
            loudness: 0.1,
        },
    ),
    (
        "Small",
        "kokoro-82m-q8_0.gguf",
        Gate {
            envelope: 0.95,
            length: 0.05,
            loudness: 0.15,
        },
    ),
    // Its coarser 8-bit file drifts as far under onnxruntime.
    (
        "Tiny",
        "kokoro-v1_0-q8.onnx",
        Gate {
            envelope: 0.8,
            length: 0.05,
            loudness: 0.15,
        },
    ),
];

/// How near a build must sound to the full weights.
struct Gate {
    /// Loudness-curve correlation at least.
    envelope: f64,
    /// Length apart at most, as a share.
    length: f64,
    /// RMS apart at most, as a share.
    loudness: f64,
}

/// Compares each build found in `dir` with `full` on `inputs`.
pub fn check(
    dir: &Path,
    out: &Path,
    (full, voice): (&ours::Kokoro, &ours::Voice),
    inputs: &[(String, Vec<u32>)],
    (report, failed): (&mut String, &mut Vec<String>),
) -> R<()> {
    writeln!(report, "## Builds against Full\n")?;
    writeln!(
        report,
        "| build | text | durations same | audio s | loudness-curve corr | \
         waveform corr | RMS / full | render s | verdict |"
    )?;
    writeln!(report, "|---|---|---|---|---|---|---|---|---|")?;
    let config = dir.join("config.json");
    let mut loads = String::new();
    for (name, file, gate) in &BUILDS {
        let path = dir.join(file);
        if !path.exists() {
            failed.push(format!("{name}: {file} missing"));
            continue;
        }
        let start = Instant::now();
        let build = ours::Kokoro::from_files(&config, &path)?;
        write!(loads, "{name} {:.2} s, ", start.elapsed().as_secs_f64())?;
        for (tag, ids) in inputs {
            let style = voice.style(ids.len())?;
            let want = full.trace(ids, &style, 1.0)?;
            let start = Instant::now();
            let got = build.trace(ids, &style, 1.0)?;
            let took = start.elapsed().as_secs_f64();
            let same = (want.durations.iter())
                .zip(&got.durations)
                .filter(|(a, b)| a == b)
                .count();
            let (a, b) = (&want.audio, &got.audio);
            let env = pearson(&envelope(b), &envelope(a));
            let wave = pearson(b, a);
            let ratio = rms(b) / rms(a).max(1e-12);
            let apart = a.len().abs_diff(b.len()) as f64 / a.len().max(1) as f64;
            let ok = env >= gate.envelope
                && apart <= gate.length
                && (ratio - 1.0).abs() <= gate.loudness;
            if !ok {
                failed.push(format!(
                    "{name} {tag}: loudness corr {env:.4}, length {apart:.3} apart, RMS ×{ratio:.3}"
                ));
            }
            writeln!(
                report,
                "| {name} | {tag} | {same}/{} | {:.2} / {:.2} | {env:.4} | {wave:.4} | {ratio:.3} | \
                 {took:.2} | {} |",
                want.durations.len(),
                seconds(b.len()),
                seconds(a.len()),
                if ok { "ok" } else { "**DIFFERS**" },
            )?;
            write_wav(
                &out.join(format!("{tag}-{}.wav", name.to_lowercase())),
                b,
                ours::SAMPLE_RATE,
            )?;
        }
    }
    writeln!(report, "\nLoading: {}.\n", loads.trim_end_matches(", "))?;
    Ok(())
}
