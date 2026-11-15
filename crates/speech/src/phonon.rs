//! Phonon as a local model: one build's weights, and a file per voice.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_audio::Stretch;
use mareader_phonon::{Noise, Pace, Phonon as Model, SAMPLE_RATE, TEMPERATURE, Voice};

use crate::local::{Line, Out, Synth};
use crate::studio::Engine;

/// The speeds offered, as other models give.
pub(crate) const SPEEDS: RangeInclusive<f32> = 0.5..=3.0;

/// Speed against its steady paces: the reference's medians over 12 lines.
const PACES: [(f32, f32); 7] = [
    (0.91, 0.2),
    (0.946, 0.1),
    (1.0, 0.0),
    (1.066, -0.1),
    (1.126, -0.2),
    (1.183, -0.3),
    (1.244, -0.4),
];

/// Within this of 1, stretching is left out.
const PLAIN: f32 = 0.01;

/// Where Phonon's build is kept, and its voices.
#[derive(Clone)]
pub(crate) struct Source {
    pub config: PathBuf,
    pub weights: PathBuf,
    pub tokenizer: PathBuf,
    /// Each downloaded voice's file, by id.
    pub voices: HashMap<String, PathBuf>,
    /// The model's name with its build's, as `Phonon (8-bit)`.
    pub name: Arc<str>,
}

/// What a voice needs once loaded.
pub(crate) struct Loaded {
    model: Arc<Model>,
    id: String,
    voice: Voice,
}

/// Phonon from `source`, speaking a voice at a time.
pub(crate) type Phonon = Engine<Source, Loaded>;

impl Phonon {
    fn load_fresh(&self, id: &str) -> Result<Loaded, String> {
        let source = &self.source;
        let path =
            (source.voices.get(id)).ok_or_else(|| format!("{} has no voice {id}", source.name))?;
        let model = (self.studio).model(&source.name, || {
            Model::load(&source.config, &source.weights, &source.tokenizer)
                .map_err(|e| e.to_string())
        })?;
        let voice = model.voice(path).map_err(|e| e.to_string())?;
        Ok(Loaded {
            model,
            id: id.to_owned(),
            voice,
        })
    }
}

impl Synth for Phonon {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        if !self.loaded.as_ref().is_some_and(|l| l.id == voice) {
            // The old voice goes first, so two are never held.
            self.loaded = None;
            self.loaded = Some(self.load_fresh(voice)?);
        }
        Ok(())
    }

    fn unload(&mut self) {
        self.loaded = None;
    }

    fn loaded(&self) -> bool {
        self.loaded.is_some()
    }

    fn render(&mut self, line: &Line<'_>, out: &mut Out<'_>) -> Result<(), String> {
        let loaded = self.loaded.as_ref().ok_or("Phonon was let go")?;
        let (pace, rest) = split(line.speed);
        // The pace gets near; same-pitch stretching does what is left.
        let mut stretch = ((rest - 1.0).abs() > PLAIN).then(|| Stretch::new(SAMPLE_RATE, 1, rest));
        let mut going = true;
        for chunk in loaded.model.chunks(line.text) {
            // Seeded, so the same line always sounds the same.
            let mut noise = Noise::new(TEMPERATURE, 0);
            let said = loaded
                .model
                .say(&loaded.voice, &chunk, pace, &mut noise, |block| {
                    going = match &mut stretch {
                        Some(s) => give(out, &s.push(&block)),
                        None => out(&block, SAMPLE_RATE, false, &[]),
                    };
                    going
                });
            said.map_err(|e| e.to_string())?;
            if !going {
                return Ok(());
            }
        }
        if let Some(s) = &mut stretch {
            give(out, &s.drain());
        }
        Ok(())
    }
}

/// Hands `out` a stretched piece; one still empty goes on quietly.
fn give(out: &mut Out<'_>, piece: &[f32]) -> bool {
    piece.is_empty() || out(piece, SAMPLE_RATE, false, &[])
}

/// The pace nearest `speed`, and the stretch that makes up the rest.
fn split(speed: f32) -> (Pace, f32) {
    let speed = match speed.is_finite() {
        true => speed.clamp(*SPEEDS.start(), *SPEEDS.end()),
        false => 1.0,
    };
    let first = PACES.first().map_or(1.0, |p| p.0);
    let last = PACES.last().map_or(1.0, |p| p.0);
    let natural = speed.clamp(first, last);
    (pace(natural), speed / natural)
}

/// The pace for `speed`, read off [`PACES`] between its points.
fn pace(speed: f32) -> Pace {
    let above = PACES.iter().position(|&(s, _)| s >= speed);
    let delta = match above.map(|i| (i.checked_sub(1).and_then(|j| PACES.get(j)), PACES.get(i))) {
        Some((Some(&(s0, d0)), Some(&(s1, d1)))) => d0 + (d1 - d0) * (speed - s0) / (s1 - s0),
        Some((None, Some(&(_, d)))) => d,
        _ => PACES.last().map_or(0.0, |&(_, d)| d),
    };
    Pace(delta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice as Listed;
    use crate::local::{Engine, Setup};
    use crate::studio::Studio;

    #[test]
    fn phonon_reads_each_speed_as_a_pace_between_the_measured_ones() {
        let near = |a: f32, b: f32| (a - b).abs() < 1e-4;
        assert!(near(pace(1.0).0, 0.0));
        assert!(near(pace(1.126).0, -0.2));
        assert!(near(pace(1.096).0, -0.15));
        assert!(near(pace(0.928).0, 0.15));
        // Past the steady paces, stretching makes up the rest.
        let (fast, rest) = split(2.0);
        assert!(near(fast.0, -0.4) && near(rest, 2.0 / 1.244));
        let (slow, rest) = split(0.5);
        assert!(near(slow.0, 0.2) && near(rest, 0.5 / 0.91));
        assert_eq!(split(1.1).1, 1.0);
        // Held to the speeds offered, and 1 when no number.
        assert_eq!(split(9.0), split(3.0));
        assert_eq!(split(f32::NAN), (Pace(0.0), 1.0));
        assert!(
            PACES
                .windows(2)
                .all(|w| matches!(w, [a, b] if a.0 < b.0 && a.1 > b.1))
        );
    }

    #[test]
    fn a_downloaded_phonon_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_PHONON") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let name: Arc<str> = "Phonon (8-bit)".into();
        let voices = [("default".to_owned(), dir.join("default-voice.safetensors"))];
        let source = Source {
            config: dir.join("config.json"),
            weights: dir.join("model.q8.gguf"),
            tokenizer: dir.join("tokenizer.json"),
            voices: voices.into_iter().collect(),
            name: Arc::clone(&name),
        };
        let setup = Setup {
            name,
            voices: vec![Listed {
                id: "default".into(),
                name: "Default".into(),
                language: "en-US".into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let alone = Phonon::new(source.clone(), Studio::default());
        crate::local::tests::time_load(alone, "default", "phonon engine");
        let studio = Studio::default();
        let synth = Phonon::new(source.clone(), studio.clone());
        match Engine::open(synth, setup, studio, Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, "phonon engine"),
            Err(e) => panic!("{e}"),
        }
        // Speeds past the steady paces still come out near what is asked.
        let mut synth = Phonon::new(source, Studio::default());
        synth.load("default").unwrap();
        let mut length = |speed: f32| {
            let text = "A quiet morning by the sea, and nobody about.";
            let line = Line {
                text,
                speed,
                knobs: Vec::new(),
                lead: false,
            };
            let mut n = 0;
            let mut count = |a: &[f32], _: u32, _: bool, _: &[crate::timing::Mark]| {
                n += a.len();
                true
            };
            synth.render(&line, &mut count).unwrap();
            n as f32
        };
        let plain = length(1.0);
        for speed in [0.5, 0.75, 1.5, 2.0, 3.0] {
            let got = plain / length(speed);
            println!("phonon engine: asked {speed}x, got {got:.3}x");
            assert!((0.75..1.33).contains(&(got / speed)), "{speed}: {got}");
        }
    }
}
