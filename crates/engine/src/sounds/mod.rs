//! How words sound on their cards, from a download voice models share.

use std::collections::HashMap;
#[cfg(not(target_arch = "wasm32"))]
use std::path::PathBuf;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, mpsc};

use mareader_g2p::Dialect;
use mareader_settings::PronunciationSettings;
use mareader_speech::{Item, PRONUNCIATION, Shelf, Stage};

use crate::dict::Download;
#[cfg(not(target_arch = "wasm32"))]
use lines::understood;
use lines::{near_tag, speakable, tag};

mod lines;

pub use lines::{
    Beat, EnglishVoice, Heard, Line, Spoken, accent, format, named, shown, spoken, stand_in,
};

/// Lookups remembered; past this the list starts over.
const KEPT: usize = 256;
/// Seconds a loaded accent stays after its last word.
#[cfg(not(target_arch = "wasm32"))]
const IDLE: f64 = 120.0;

type Key = (String, Dialect);

#[cfg(not(target_arch = "wasm32"))]
type Answer = Result<(String, Arc<mareader_g2p::G2p>), String>;

/// A lookup away from the frame.
#[cfg(not(target_arch = "wasm32"))]
struct Asking {
    key: Key,
    answer: mpsc::Receiver<Answer>,
}

/// The pronunciation download and the words looked up in it.
#[derive(Default)]
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub struct Sounds {
    shelf: Shelf,
    /// Each word's phonemes per accent; None when nothing could say it.
    found: HashMap<Key, Option<String>>,
    wanted: Vec<Key>,
    /// Lookups run on the frame, for tests.
    inline: bool,
    #[cfg(not(target_arch = "wasm32"))]
    lexicons: Option<mareader_speech::Lexicons>,
    #[cfg(not(target_arch = "wasm32"))]
    waker: Option<mareader_speech::Waker>,
    #[cfg(not(target_arch = "wasm32"))]
    asking: Option<Asking>,
    /// Loaded accents, kept while words come; dropped when idle.
    #[cfg(not(target_arch = "wasm32"))]
    held: Vec<Arc<mareader_g2p::G2p>>,
    used: f64,
}

impl Sounds {
    /// Keeps the download beside the voice models in `dir`, sharing files.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn attach(
        &mut self,
        dir: PathBuf,
        waker: mareader_speech::Waker,
        voices: &mut Shelf,
    ) -> Result<(), String> {
        self.shelf
            .attach(dir, mareader_speech::AIDS, waker.clone())?;
        self.shelf.share_with(voices);
        self.lexicons = Some(voices.lexicons());
        self.waker = Some(waker);
        self.shelf.adopt();
        Ok(())
    }

    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
    }

    pub fn attached(&self) -> bool {
        self.shelf.attached()
    }

    pub fn stage(&self) -> &Stage {
        self.shelf.stage(Item::Model(PRONUNCIATION.id))
    }

    pub fn ready(&self) -> bool {
        *self.stage() == Stage::Ready
    }

    /// The download's size.
    pub fn bytes(&self) -> u64 {
        PRONUNCIATION.bytes()
    }

    /// Voice models holding the same files; removing the pack frees nothing.
    pub fn shared_with(&self) -> Vec<&'static str> {
        let holders = self.shelf.holders(&PRONUNCIATION);
        holders.iter().map(|m| m.name).collect()
    }

    pub fn act(&mut self, action: Download) -> Result<(), String> {
        let item = Item::Model(PRONUNCIATION.id);
        #[cfg(not(target_arch = "wasm32"))]
        match action {
            Download::Start => self.shelf.download(item),
            Download::Pause => self.shelf.pause(item),
            Download::Cancel => self.shelf.cancel(item),
            Download::Remove => {
                self.forget();
                return self.shelf.remove(item);
            }
        }
        #[cfg(target_arch = "wasm32")]
        let _ = (item, action);
        Ok(())
    }

    /// A voice model came or went: the pack takes in what it holds.
    pub(crate) fn voices_changed(&mut self) {
        self.shelf.adopt();
    }

    /// Takes in downloads and answers; true when something changed.
    pub fn poll(&mut self, now: f64) -> bool {
        let changed = self.shelf.poll();
        #[cfg(not(target_arch = "wasm32"))]
        let changed = {
            let answered = self.receive();
            self.pump();
            if self.asking.is_none() && now - self.used > IDLE {
                self.held.clear();
            }
            changed | answered
        };
        #[cfg(target_arch = "wasm32")]
        let _ = now;
        changed
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn forget(&mut self) {
        self.found.clear();
        self.wanted.clear();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.asking = None;
            self.held.clear();
        }
    }

    /// How `word` sounds as `settings` asks, once it has been looked up.
    pub fn lines(
        &mut self,
        word: &str,
        (settings, voice): (&PronunciationSettings, Spoken),
        now: f64,
    ) -> Option<Vec<Line>> {
        let word = word.trim();
        if !settings.shown || !self.ready() || !speakable(word) {
            return None;
        }
        let dialects = shown(settings, voice);
        let near = match voice {
            Spoken::Nearest { english, .. } if settings.follow_voice => Some(english),
            _ => None,
        };
        let mut lines = Vec::new();
        for &dialect in dialects {
            let key = (word.to_owned(), dialect);
            match self.found.get(&key) {
                Some(Some(phonemes)) => {
                    let mut line = format(phonemes, dialect, settings.notation);
                    line.tag = match near {
                        _ if dialects.len() > 1 => tag(dialect),
                        Some(_) => near_tag(dialect),
                        None => "",
                    };
                    line.why = near.map(|english| stand_in(english, dialect));
                    lines.push(line);
                }
                Some(None) => {}
                None => self.want(key, now),
            }
        }
        (!lines.is_empty()).then_some(lines)
    }

    fn want(&mut self, key: Key, now: f64) {
        self.used = now;
        #[cfg(not(target_arch = "wasm32"))]
        let busy = self.asking.as_ref().is_some_and(|a| a.key == key);
        #[cfg(target_arch = "wasm32")]
        let busy = false;
        if !busy && !self.wanted.contains(&key) {
            self.wanted.push(key);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.pump();
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn keep(&mut self, key: Key, phonemes: Option<String>) {
        if self.found.len() >= KEPT {
            self.found.clear();
        }
        self.found.insert(key, phonemes);
    }

    /// Starts the next lookup when none runs.
    #[cfg(not(target_arch = "wasm32"))]
    fn pump(&mut self) {
        while self.asking.is_none() && !self.wanted.is_empty() {
            let key = self.wanted.remove(0);
            let (Some(lexicons), Some(dir)) =
                (self.lexicons.clone(), self.shelf.folder(&PRONUNCIATION))
            else {
                self.keep(key, None);
                continue;
            };
            let (tx, rx) = mpsc::channel();
            let (word, dialect) = (key.0.clone(), key.1);
            let job = move || {
                let g2p = lexicons.load(&dir, dialect)?;
                Ok((g2p.phonemize(&word), g2p))
            };
            self.asking = Some(Asking { key, answer: rx });
            if self.inline {
                let _ = tx.send(job());
                self.receive();
                continue;
            }
            let waker = self.waker.clone();
            std::thread::spawn(move || {
                let _ = tx.send(job());
                if let Some(wake) = waker {
                    wake();
                }
            });
        }
    }

    /// Takes in a finished lookup.
    #[cfg(not(target_arch = "wasm32"))]
    fn receive(&mut self) -> bool {
        let Some(asking) = &self.asking else {
            return false;
        };
        let answer = match asking.answer.try_recv() {
            Ok(answer) => answer,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => Err("stopped".into()),
        };
        let key = asking.key.clone();
        self.asking = None;
        let phonemes = match answer {
            Ok((said, g2p)) => {
                if !self.held.iter().any(|h| Arc::ptr_eq(h, &g2p)) {
                    self.held.push(g2p);
                }
                understood(&said)
            }
            // Files that won't load sound out nothing.
            Err(_) => None,
        };
        self.keep(key, phonemes);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_shows_before_the_pack_is_in() {
        let mut sounds = Sounds::default();
        let shown = PronunciationSettings::default();
        let unsure = (&shown, Spoken::Unsure);
        assert_eq!(sounds.lines("yacht", unsure, 0.0), None);
        assert!(sounds.wanted.is_empty());
    }
}
