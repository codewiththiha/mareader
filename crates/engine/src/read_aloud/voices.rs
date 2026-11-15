//! Voice listing, speaker matching, and voice-booth selection for read-aloud.

use mareader_settings::ReadAloudSettings;
use mareader_speech::{Model, Voice};

use super::state::State;
use crate::sounds::EnglishVoice;

/// Who says a word in one language: the picked voice, or another one.
#[derive(Clone, Debug, PartialEq)]
pub struct Speaker {
    /// As `fr`.
    pub language: String,
    /// None for the voice picked for the engine.
    pub voice: Option<String>,
}

/// A language the engine's model has voices for, none downloaded yet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Offer {
    pub model: &'static Model,
    /// Its full tag, as the model's voices name it: `fr-FR`.
    pub tag: &'static str,
}

impl State {
    /// Keeps the settings booth open for `engine`, or closes it.
    pub fn voices_shown(&mut self, engine: Option<&str>, settings: &ReadAloudSettings) {
        if let Some(engine) = engine {
            self.booth.open(engine, settings, &self.shelf);
            self.booth.tick();
        } else if self.booth.is_open() {
            self.booth.close();
        }
    }

    /// Opens a system engine once to list its voices while none are known.
    pub fn list_voices(&mut self, settings: &ReadAloudSettings) {
        let engine = settings.engine.as_str();
        let model = mareader_speech::MODELS.iter().any(|m| m.id == engine);
        let tried = self.listed_once.iter().any(|e| e == engine);
        if model || tried || !self.listed(engine).is_empty() {
            return;
        }
        self.listed_once.push(engine.to_owned());
        self.word.open(engine, settings, &self.shelf);
        self.keep_listing();
    }

    /// Who says English words on cards, as [`Self::speakers`] picks.
    pub fn english_voice(
        &self,
        settings: &ReadAloudSettings,
        others: bool,
    ) -> Option<EnglishVoice> {
        let english = |tag: &str| mareader_speech::primary(tag) == "en";
        let own = self.own_voice(settings);
        if english(&own.tag) {
            return Some(own);
        }
        if !others {
            return None;
        }
        let engine = settings.engine.as_str();
        let found = match mareader_speech::MODELS.iter().find(|m| m.id == engine) {
            Some(m) => (m.voices.iter())
                .find(|v| english(v.language) && self.shelf.has(m, v.id))
                .map(|v| (v.name.to_owned(), v.language.to_owned())),
            None => (self.listed(engine).iter())
                .find(|v| english(&v.language))
                .map(|v| (v.name.clone(), v.language.clone())),
        };
        found.map(|(name, tag)| EnglishVoice { name, tag })
    }

    /// Who says each language's words; `others` lends other installed voices.
    pub fn speakers(
        &self,
        settings: &ReadAloudSettings,
        others: bool,
    ) -> (Vec<Speaker>, Vec<Offer>) {
        let own = Speaker {
            language: mareader_speech::primary(&self.own_voice(settings).tag),
            voice: None,
        };
        let mut speakers = vec![own];
        let mut offers: Vec<Offer> = Vec::new();
        if !others {
            return (speakers, offers);
        }
        let engine = settings.engine.as_str();
        let primary = mareader_speech::primary;
        let known = |all: &[Speaker], language: &str| all.iter().any(|s| s.language == language);
        let Some(model) = mareader_speech::MODELS.iter().find(|m| m.id == engine) else {
            for v in self.listed(engine) {
                let language = primary(&v.language);
                if !known(&speakers, &language) {
                    let voice = Some(v.id.clone());
                    speakers.push(Speaker { language, voice });
                }
            }
            return (speakers, offers);
        };
        for v in model.voices.iter().filter(|v| self.shelf.has(model, v.id)) {
            let language = primary(v.language);
            if !known(&speakers, &language) {
                let voice = Some(v.id.to_owned());
                speakers.push(Speaker { language, voice });
            }
        }
        let here = self.shelf.has(model, model.first_voice);
        for v in model.voices.iter().filter(|_| here) {
            let language = primary(v.language);
            let offered = offers.iter().any(|o| primary(o.tag) == language);
            if !known(&speakers, &language) && !offered {
                offers.push(Offer {
                    model,
                    tag: v.language,
                });
            }
        }
        (speakers, offers)
    }

    fn own_voice(&self, settings: &ReadAloudSettings) -> EnglishVoice {
        let engine = settings.engine.as_str();
        let picked = settings.voice_for(engine);
        let model = mareader_speech::MODELS.iter().find(|m| m.id == engine);
        let voice = match model {
            Some(m) => picked.and_then(|id| m.voice(id)).or(m.first()),
            None => None,
        };
        let voice = voice.map(|v| (v.name.to_owned(), v.language.to_owned()));
        let voice = voice.or_else(|| {
            let id = picked?;
            let v = self.listed(engine).iter().find(|v| v.id == id)?;
            Some((v.name.clone(), v.language.clone()))
        });
        let (name, tag) = voice.unwrap_or_else(|| {
            let home = mareader_speech::home_language();
            ("Your system voice".to_owned(), home)
        });
        EnglishVoice { name, tag }
    }

    fn listed(&self, engine: &str) -> &[Voice] {
        let open = [&self.word, &self.booth]
            .into_iter()
            .find(|b| b.engine() == engine && !b.voices().is_empty());
        match open {
            Some(b) => b.voices(),
            None if self.known.0 == engine => &self.known.1,
            None => &[],
        }
    }

    pub(super) fn keep_listing(&mut self) {
        for b in [&self.word, &self.booth] {
            let (engine, voices) = (b.engine(), b.voices());
            let changed = self.known.0 != engine || self.known.1.len() != voices.len();
            if !voices.is_empty() && changed {
                self.known = (engine.to_owned(), voices.to_vec());
            }
        }
    }
}
