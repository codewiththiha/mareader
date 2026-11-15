//! Kitten as a local model: a build's weights, its voices, and the G2P.

use std::collections::HashMap;
use std::ops::RangeInclusive;
use std::path::PathBuf;
use std::sync::Arc;

use mareader_g2p::{Dialect, G2p};
use mareader_kokoro::{Kokoro as Model, SAMPLE_RATE, Voice, kitten};
use mareader_vits::{Style, espeak_like};

use crate::local::{Line, Out, Synth, both, trim_join};
use crate::piper::pieces;
use crate::studio::Engine;
use crate::timing::{Words, offsets, place, shift, spell};

/// The speeds offered, as shares of each voice's own pace.
pub(crate) const SPEEDS: RangeInclusive<f32> = 0.5..=3.0;
/// The pause owed at a clause split, in seconds at normal speed.
const CLAUSE: f64 = 0.12;

/// Where a Kitten build is kept, and how it speaks.
#[derive(Clone)]
pub(crate) struct Source {
    /// Where the G2P's files are.
    pub folder: PathBuf,
    pub weights: PathBuf,
    pub voices: PathBuf,
    /// The model's name with its build's, as `Kitten (Nano)`.
    pub name: Arc<str>,
    /// Nano was tuned to be slowed a little, as its own settings say.
    pub nano: bool,
}

/// A build's weights and voices, held as one.
struct Build {
    model: Model,
    voices: HashMap<String, Voice>,
}

/// What a voice needs once loaded.
pub(crate) struct Loaded {
    build: Arc<Build>,
    g2p: Arc<G2p>,
    voice: String,
}

/// Kitten's build from `source`, speaking a voice at a time.
pub(crate) type Kitten = Engine<Source, Loaded>;

impl Kitten {
    fn load_fresh(&self, voice: &str) -> Result<Loaded, String> {
        let source = &self.source;
        let build = || {
            (self.studio).model(&source.name, || {
                let model = Model::from_onnx(&source.weights).map_err(|e| e.to_string())?;
                let voices = kitten::read_voices(&source.voices).map_err(|e| e.to_string())?;
                Ok(Build {
                    model,
                    voices: voices.into_iter().collect(),
                })
            })
        };
        let g2p = || {
            (self.studio).g2p(Dialect::American, || {
                G2p::load(&source.folder, Dialect::American).map_err(|e| e.to_string())
            })
        };
        let (build, g2p) = both(build, g2p)?;
        if !build.voices.contains_key(voice) {
            return Err(format!("{} has no voice {voice}", source.name));
        }
        Ok(Loaded {
            build,
            g2p,
            voice: voice.to_owned(),
        })
    }
}

impl Synth for Kitten {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        if !self.loaded.as_ref().is_some_and(|l| l.voice == voice) {
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
        let loaded = self.loaded.as_ref().ok_or("Kitten was let go")?;
        let voice = (loaded.build.voices.get(&loaded.voice)).ok_or("Kitten lost its voice")?;
        let speed =
            line.speed.clamp(*SPEEDS.start(), *SPEEDS.end()) * prior(&self.source, &loaded.voice);
        let tokens = loaded.g2p.spelt(line.text);
        let spelt = spell(
            line.text,
            tokens
                .iter()
                .map(|t| (&*t.text, &*t.phonemes, &*t.whitespace)),
        );
        let phonemes = espeak_like(&spelt.phonemes, Style::American);
        let words = Words::new(&phonemes, &spelt);
        let pieces = pieces(&phonemes, line.lead);
        let starts = offsets(&phonemes, &pieces);
        for ((i, piece), from) in pieces.iter().enumerate().zip(starts) {
            let here = words.within(from, from + piece.chars().count());
            let piece = closed(piece);
            let style = kitten::style(voice, &piece).map_err(|e| e.to_string())?;
            let ids = kitten::ids(&piece);
            let trace =
                (loaded.build.model.trace(&ids, &style, speed)).map_err(|e| e.to_string())?;
            let mut audio = trace.audio;
            let id_at = kitten::ids_at(&piece);
            let mut marks = place(&here, &id_at, &trace.durations, audio.len());
            trim(&mut audio);
            let last = i + 1 == pieces.len();
            let owed = CLAUSE / f64::from(line.speed);
            let cut = trim_join(&mut audio, SAMPLE_RATE, i > 0, (!last).then_some(owed));
            shift(&mut marks, cut, audio.len());
            if !out(&audio, SAMPLE_RATE, last, &marks) {
                break;
            }
        }
        Ok(())
    }
}

/// The pace a voice was tuned for in this build, as its settings give it.
fn prior(source: &Source, voice: &str) -> f32 {
    match (source.nano, voice) {
        (true, "expr-voice-4-m") => 0.9,
        (true, _) => 0.8,
        (false, _) => 1.0,
    }
}

/// A piece ending in a mark, as Kitten closes every run it is given.
fn closed(piece: &str) -> String {
    let marked = piece.ends_with(['.', ',', '!', '?', ';', ':', '…']);
    match marked {
        true => piece.to_owned(),
        false => format!("{piece},"),
    }
}

/// Cuts the tail Kitten leaves after each run; a short run keeps half.
fn trim(audio: &mut Vec<f32>) {
    let keep = audio
        .len()
        .saturating_sub(kitten::TRIM)
        .max(audio.len() / 2);
    audio.truncate(keep);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice as Listed;
    use crate::local::{Engine, Setup};
    use crate::studio::Studio;

    fn source(nano: bool) -> Source {
        Source {
            folder: PathBuf::new(),
            weights: PathBuf::new(),
            voices: PathBuf::new(),
            name: "Kitten (Nano)".into(),
            nano,
        }
    }

    #[test]
    fn kitten_slows_nano_as_tuned_and_closes_each_piece() {
        assert_eq!(prior(&source(true), "expr-voice-2-f"), 0.8);
        assert_eq!(prior(&source(true), "expr-voice-4-m"), 0.9);
        assert_eq!(prior(&source(false), "expr-voice-4-m"), 1.0);
        assert_eq!(closed("həlˈoʊ"), "həlˈoʊ,");
        assert_eq!(closed("həlˈoʊ."), "həlˈoʊ.");
        let mut long = vec![0.5; kitten::TRIM + 100];
        trim(&mut long);
        assert_eq!(long.len(), 100.max((kitten::TRIM + 100) / 2));
        let mut short = vec![0.5; 10];
        trim(&mut short);
        assert_eq!(short.len(), 5);
    }

    #[test]
    fn a_downloaded_kitten_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KITTEN") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let name: Arc<str> = "Kitten (Nano)".into();
        let source = Source {
            weights: dir.join("nano.onnx"),
            voices: dir.join("nano-voices.npz"),
            folder: dir,
            name: Arc::clone(&name),
            nano: true,
        };
        let setup = Setup {
            name,
            voices: vec![Listed {
                id: "expr-voice-2-f".into(),
                name: "Bella".into(),
                language: "en-US".into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let alone = Kitten::new(source.clone(), Studio::default());
        crate::local::tests::time_load(alone, "expr-voice-2-f", "kitten engine");
        let studio = Studio::default();
        let synth = Kitten::new(source, studio.clone());
        match Engine::open(synth, setup, studio, Arc::new(|| {})) {
            Ok(e) => crate::local::tests::live(e, "kitten engine"),
            Err(e) => panic!("{e}"),
        }
    }

    /// The process's resident memory, in MB, where the system tells it.
    fn resident_mb() -> f64 {
        let pid = std::process::id().to_string();
        #[cfg(target_os = "linux")]
        let kb = std::fs::read_to_string("/proc/self/status")
            .ok()
            .and_then(|s| {
                let line = s.lines().find(|l| l.starts_with("VmRSS:"))?;
                line.split_whitespace().nth(1)?.parse::<f64>().ok()
            });
        #[cfg(target_os = "macos")]
        let kb = std::process::Command::new("ps")
            .args(["-o", "rss=", "-p", &pid])
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<f64>()
                    .ok()
            });
        #[cfg(windows)]
        let kb = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command"])
            .arg(format!("(Get-Process -Id {pid}).WorkingSet64"))
            .output()
            .ok()
            .and_then(|o| {
                String::from_utf8_lossy(&o.stdout)
                    .trim()
                    .parse::<f64>()
                    .ok()
            })
            .map(|bytes| bytes / 1024.0);
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        let kb: Option<f64> = None;
        let _ = pid;
        kb.map_or(0.0, |kb| (kb / 1024.0).round())
    }

    /// Resident megabytes once the thread letting go has handed it back.
    fn settled_mb() -> f64 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        resident_mb()
    }

    /// Seconds until `done`, or None after `most`.
    fn waited(most: u64, done: impl Fn() -> bool) -> Option<f64> {
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_secs(most) {
            if done() {
                return Some(start.elapsed().as_secs_f64());
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        None
    }

    #[test]
    fn a_kitten_let_go_and_removed_leaves_memory_and_disk() {
        use crate::{Item, Shelf, Stage, Voicer};
        use std::time::Duration;
        // Only where a downloaded shelf is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KITTEN_SHELF") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let folder = dir.join("kitten");
        let mut shelf = Shelf::default();
        shelf.attach(dir, crate::MODELS, Arc::new(|| {})).unwrap();
        assert_eq!(*shelf.stage(Item::Build("kitten", "mini")), Stage::Ready);
        let loaded = |shelf: &Shelf| !shelf.usage().models.is_empty();
        let mut voicer: Box<dyn Voicer> =
            crate::open("kitten", Some("mini"), &shelf, Arc::new(|| {})).unwrap();
        let mut said = 0;
        let mut say = |voicer: &mut Box<dyn Voicer>, shelf: &Shelf, text: &str| {
            said += 1;
            let made = shelf.usage().speech;
            voicer.say(said, text).unwrap();
            let took = waited(600, || shelf.usage().speech > made);
            assert!(took.is_some(), "{text:?} was never made");
        };
        let start = resident_mb();
        println!("kitten memory: {start} MB before any model");
        shelf.set_model_idle(Some(Duration::from_secs(1)));
        for round in 1..=2 {
            let text = format!("This is round {round}, read aloud to load the model.");
            say(&mut voicer, &shelf, &text);
            let held = resident_mb();
            // No sound device here, so reading only ends when stopped.
            voicer.stop();
            let gone = waited(60, || !loaded(&shelf));
            let after = settled_mb();
            println!(
                "kitten memory: round {round}: {held} MB loaded, \
                 let go after {gone:?} s quiet, {after} MB after"
            );
            assert!(gone.is_some(), "round {round}: still loaded");
        }
        // While reading: kept as long as it reads, then half a minute.
        shelf.set_model_idle(None);
        say(
            &mut voicer,
            &shelf,
            "Now it should stay loaded while reading.",
        );
        std::thread::sleep(Duration::from_secs(3));
        assert!(loaded(&shelf), "let go in the middle of reading");
        voicer.pause();
        let gone = waited(60, || !loaded(&shelf));
        println!("kitten memory: while reading, let go {gone:?} s after pause");
        assert!(gone.is_some_and(|s| s > 20.0), "{gone:?} s after pause");
        // A limit changed applies at once, not at the next look.
        voicer.stop();
        shelf.set_model_idle(Some(Duration::from_secs(600)));
        say(&mut voicer, &shelf, "Loaded once more, then removed.");
        voicer.stop();
        // Removed while loaded: the files go at once.
        shelf.remove(Item::Model("kitten")).unwrap();
        println!(
            "kitten disk: removed while loaded, folder left: {}",
            folder.exists()
        );
        assert!(!folder.exists(), "the folder outlived its removal");
        assert_eq!(*shelf.stage(Item::Build("kitten", "mini")), Stage::Absent);
        std::thread::sleep(Duration::from_secs(2));
        assert!(loaded(&shelf), "let go before its ten minutes");
        shelf.set_model_idle(Some(Duration::from_secs(1)));
        let gone = waited(10, || !loaded(&shelf));
        println!(
            "kitten memory: limit cut to 1 s, let go after {gone:?} s, {} MB",
            settled_mb()
        );
        assert!(gone.is_some_and(|s| s < 5.0), "{gone:?} s after the cut");
        drop(voicer);
        std::thread::sleep(Duration::from_secs(1));
        println!("kitten memory: engine closed, {} MB", resident_mb());
    }
}
