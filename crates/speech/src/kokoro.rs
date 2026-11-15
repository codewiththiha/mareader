//! Kokoro as a local model: its weights, a voice pack, and the G2P.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mareader_g2p::{Dialect, G2p, PhonemeToken};
use mareader_hanzi::Hanzi;
use mareader_ipa::Language;
use mareader_kokoro::{Kokoro as Model, SAMPLE_RATE, Voice as Pack};

use crate::local::{Line, Out, Synth, both, trim_join};
use crate::studio::Engine;
use crate::timing::{Words, place, shift, spell};

pub(crate) use mareader_kokoro::SPEEDS;

/// Tokens in the first piece of a sentence wanted now, about two seconds.
const LEAD: usize = 32;

/// Where a Kokoro build is kept, and its name.
#[derive(Clone)]
pub(crate) struct Source {
    pub folder: PathBuf,
    pub weights: PathBuf,
    /// The model's name with its build's, as `Kokoro (Small)`.
    pub name: Arc<str>,
}

/// What a voice needs once loaded.
pub(crate) struct Loaded {
    model: Arc<Model>,
    reader: Reader,
    voice: (String, Pack),
}

/// What turns a voice's text into phonemes.
enum Reader {
    /// English, from the downloaded G2P data.
    English(Arc<G2p>),
    /// Another language, by spelling rules alone.
    Rules(Language),
    /// Chinese, from the dictionaries Chinese voices download.
    Chinese(Arc<Hanzi>),
}

/// Kokoro's build from `source`, speaking a voice at a time.
pub(crate) type Kokoro = Engine<Source, Loaded>;

impl Kokoro {
    fn load_fresh(&self, voice: &str) -> Result<Loaded, String> {
        let Source {
            folder,
            weights,
            name,
        } = &self.source;
        let model = || {
            (self.studio).model(name, || {
                let config = folder.join("config.json");
                Model::from_files(&config, weights).map_err(|e| e.to_string())
            })
        };
        let rest = || {
            let pack = Pack::load(&pack_path(folder, voice)).map_err(|e| e.to_string())?;
            Ok((self.reader(voice)?, pack))
        };
        let (model, (reader, pack)) = both(model, rest)?;
        Ok(Loaded {
            model,
            reader,
            voice: (voice.to_owned(), pack),
        })
    }

    /// What reads `voice`'s language, loaded once for every voice.
    fn reader(&self, voice: &str) -> Result<Reader, String> {
        let folder = &self.source.folder;
        let chinese = voice.starts_with('z');
        Ok(match Language::for_kokoro_voice(voice) {
            Some(language) => Reader::Rules(language),
            None if chinese => Reader::Chinese(
                (self.studio)
                    .hanzi(|| Hanzi::load(&folder.join("hanzi")).map_err(|e| e.to_string()))?,
            ),
            None => {
                let dialect = Dialect::for_kokoro_voice(voice).unwrap_or(Dialect::American);
                Reader::English((self.studio).g2p(dialect, || {
                    G2p::load(folder, dialect).map_err(|e| e.to_string())
                })?)
            }
        })
    }
}

impl Synth for Kokoro {
    fn cold(&self, _voice: &str) -> bool {
        !self.studio.has_model(&self.source.name)
    }

    fn load(&mut self, voice: &str) -> Result<(), String> {
        let fits = |l: &Loaded| l.voice.0 == voice;
        if !self.loaded.as_ref().is_some_and(fits) {
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
        let loaded = self.loaded.as_ref();
        let Loaded {
            model,
            reader,
            voice,
        } = loaded.ok_or("Kokoro was let go")?;
        let (phonemes, spelt) = match reader {
            Reader::English(g2p) => {
                let tokens = g2p.tokens(line.text);
                let space = |t: &PhonemeToken| if t.whitespace { " " } else { "" };
                let spelt = spell(
                    line.text,
                    tokens.iter().map(|t| (&*t.text, &*t.phonemes, space(t))),
                );
                (spelt.phonemes.clone(), Some(spelt))
            }
            Reader::Rules(language) => (mareader_ipa::kokoro(*language, line.text), None),
            Reader::Chinese(hanzi) => (hanzi.phonemize(line.text), None),
        };
        let phonemes = model.normalise(&phonemes);
        // Only English knows its words; other readers light whole sentences.
        let words = spelt.map(|s| Words::new(&phonemes, &s));
        let chunks = match line.lead {
            true => model.chunks_led(&phonemes, LEAD),
            false => model.chunks(&phonemes),
        };
        let mut from = 0;
        for (i, chunk) in chunks.iter().enumerate() {
            let timed =
                (model.render_timed(chunk, &voice.1, line.speed)).map_err(|e| e.to_string())?;
            let count = chunk.ids.len();
            // Ids follow the characters one to one, after a leading pad.
            let id_at: Vec<usize> = (1..=count).collect();
            let here = words.as_ref().map(|w| w.within(from, from + count));
            let here = here.unwrap_or_default();
            let mut marks = place(&here, &id_at, &timed.frames, timed.voiced);
            from += count;
            let mut audio = timed.audio;
            let owed = f64::from(chunk.pause_ms) / 1000.0 / f64::from(line.speed);
            let last = i + 1 == chunks.len();
            let cut = trim_join(&mut audio, SAMPLE_RATE, i > 0, (!last).then_some(owed));
            shift(&mut marks, cut, audio.len());
            if !out(&audio, SAMPLE_RATE, last, &marks) {
                break;
            }
        }
        Ok(())
    }
}

fn pack_path(folder: &Path, voice: &str) -> PathBuf {
    folder.join("voices").join(format!("{voice}.pt"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Voice;
    use crate::local::{Engine, Setup};
    use crate::studio::Studio;

    #[test]
    fn chinese_voices_bring_what_the_reader_loads() {
        let paths: Vec<String> = mareader_hanzi::FILES
            .iter()
            .map(|f| format!("hanzi/{f}"))
            .collect();
        let listed: Vec<&str> = crate::models::HANZI.iter().map(|f| f.path).collect();
        assert_eq!(listed, paths);
        let voices = crate::models::KOKORO.voices.iter();
        let chinese: Vec<&str> = voices
            .filter(|v| v.language == "zh-CN")
            .map(|v| v.id)
            .collect();
        assert_eq!(chinese.len(), 8);
        assert!(chinese.iter().all(|id| id.starts_with('z')));
    }

    #[test]
    fn a_downloaded_model_makes_the_sentence_said_then_the_next() {
        // Only where a model folder is given, as the remote check does.
        let Some(dir) = std::env::var_os("MAREADER_KOKORO") else {
            return;
        };
        let dir = PathBuf::from(dir);
        let english = ("af_heart", "Heart", "en-US");
        time_load(&dir, english.0, "kokoro engine");
        crate::local::tests::live(engine(&dir, english), "kokoro engine");
        // Chinese reads from the dictionaries its voices download.
        if dir.join("hanzi").is_dir() {
            let what = "kokoro engine in Chinese";
            time_load(&dir, "zf_xiaobei", what);
            let chinese = engine(&dir, ("zf_xiaobei", "Xiaobei", "zh-CN"));
            let lines = (
                "我们今天去北京天安门，看看那里的风景。",
                "然后我们一起回家吃饭。",
            );
            crate::local::tests::live_saying(chinese, what, lines);
        }
    }

    /// The full build in `dir`, as the engine tests read it.
    fn source(dir: &Path) -> Source {
        Source {
            weights: dir.join(mareader_kokoro::WEIGHTS),
            folder: dir.to_owned(),
            name: "Kokoro (Full)".into(),
        }
    }

    /// Times a fresh Kokoro of its own loading `voice`.
    fn time_load(dir: &Path, voice: &str, what: &str) {
        let synth = Kokoro::new(source(dir), Studio::default());
        crate::local::tests::time_load(synth, voice, what);
    }

    /// Kokoro's engine over the model in `dir`, with only `voice`.
    fn engine(dir: &Path, (id, voice, language): (&str, &str, &str)) -> Engine {
        let source = source(dir);
        let setup = Setup {
            name: Arc::clone(&source.name),
            voices: vec![Voice {
                id: id.into(),
                name: voice.into(),
                language: language.into(),
            }],
            speeds: SPEEDS,
            knobs: &[],
        };
        let studio = Studio::default();
        let synth = Kokoro::new(source, studio.clone());
        match Engine::open(synth, setup, studio, std::sync::Arc::new(|| {})) {
            Ok(e) => e,
            Err(e) => panic!("{e}"),
        }
    }
}
