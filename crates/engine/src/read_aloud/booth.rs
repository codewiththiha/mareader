//! The voice tabs' booth: an engine opened to list and try voices.

use std::sync::Arc;

use mareader_settings::ReadAloudSettings;
use mareader_speech::{Caps, Event, Shelf, Voice, Voicer, Waker};

use super::{TICK, tone};
use crate::repaint::Repaint;

/// Opened while a voice tab shows; let go once it closes.
#[derive(Default)]
pub struct Booth {
    engine: String,
    /// The model build opened, for a model's engine.
    build: Option<&'static str>,
    voicer: Option<Box<dyn Voicer>>,
    voices: Vec<Voice>,
    /// The engine's count of changes to its voices when last listed.
    seen: u64,
    caps: Caps,
    fault: Option<String>,
    /// The sample being said, by its id; ids count every sample.
    trying: Option<u64>,
    /// The voice asked for by name, not the one picked.
    tried: Option<String>,
    said: u64,
    /// Asks the window for frames; kept across a close.
    repaint: Repaint,
}

impl Booth {
    /// Keeps `repaint`, which an engine opened here is woken through.
    pub fn attach(&mut self, repaint: Repaint) {
        self.repaint = repaint;
    }

    /// Opens `engine` in its picked build unless open, or failed, already.
    pub fn open(&mut self, engine: &str, s: &ReadAloudSettings, shelf: &Shelf) {
        let tried = self.voicer.is_some() || self.fault.is_some();
        let wanted = s.build_for(engine);
        let build = mareader_speech::build_of(engine, wanted, shelf);
        if self.engine == engine && self.build == build && tried {
            return;
        }
        self.start(engine.to_owned(), build, shelf);
    }

    fn start(&mut self, engine: String, build: Option<&'static str>, shelf: &Shelf) {
        self.close();
        self.engine = engine;
        self.build = build;
        let repaint = self.repaint.clone();
        let wake: Waker = Arc::new(move || repaint.now());
        match mareader_speech::open(&self.engine, build, shelf, wake) {
            Ok(voicer) => {
                self.caps = voicer.caps();
                self.voicer = Some(voicer);
                self.relist();
            }
            Err(fault) => self.fault = Some(fault.to_string()),
        }
    }

    /// Lists the engine's voices again, by language then name.
    fn relist(&mut self) {
        let Some(v) = self.voicer.as_ref() else {
            return;
        };
        self.seen = v.voices_seen();
        self.voices = v.voices();
        (self.voices)
            .sort_by(|a, b| (a.language.as_str(), a.name.as_str()).cmp(&(&b.language, &b.name)));
    }

    /// Stops any sample and lets the engine go; frames and count stay.
    pub fn close(&mut self) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        let (said, repaint) = (self.said, self.repaint.clone());
        *self = Self::default();
        self.said = said;
        self.repaint = repaint;
    }

    pub fn is_open(&self) -> bool {
        self.voicer.is_some() || self.fault.is_some()
    }

    /// Open and able to speak.
    pub fn ready(&self) -> bool {
        self.voicer.is_some()
    }

    /// Says a line in the voice and tone `settings` pick.
    pub fn try_voice(&mut self, settings: &ReadAloudSettings) {
        let picked = settings.voice_for(&self.engine);
        self.say_sample(settings, picked);
        self.tried = None;
    }

    /// Says a line in the voice `id`, in the tone `settings` pick.
    pub fn try_voice_of(&mut self, settings: &ReadAloudSettings, shelf: &Shelf, id: &str) {
        // Downloaded since it opened: the engine lists it once reopened.
        if self.ready() && !self.voices.iter().any(|v| v.id == id) {
            self.start(self.engine.clone(), self.build, shelf);
        }
        self.say_sample(settings, Some(id));
        self.tried = self.trying.map(|_| id.to_owned());
    }

    /// Says `text` in the voice and tone `settings` pick.
    pub fn say(&mut self, settings: &ReadAloudSettings, text: &str) {
        self.say_as(settings, None, text);
    }

    /// Says `text` in `voice`, or the one picked for the engine.
    pub fn say_as(&mut self, settings: &ReadAloudSettings, voice: Option<&str>, text: &str) {
        let picked = voice.or(settings.voice_for(&self.engine));
        self.speak(settings, picked, text);
        self.tried = None;
    }

    fn say_sample(&mut self, settings: &ReadAloudSettings, picked: Option<&str>) {
        let language = picked
            .and_then(|id| self.voices.iter().find(|v| v.id == id))
            .map_or("en", |v| v.language.as_str());
        let line = mareader_speech::sample(language);
        self.speak(settings, picked, line);
    }

    fn speak(&mut self, settings: &ReadAloudSettings, picked: Option<&str>, line: &str) {
        let Some(v) = self.voicer.as_mut() else {
            return;
        };
        v.set_voice(picked);
        v.set_tone(tone(settings));
        v.set_knobs(&settings.knobs_for(&self.engine));
        self.said = self.said.wrapping_add(1);
        match v.say(self.said, line) {
            Ok(()) => {
                self.trying = Some(self.said);
                self.fault = None;
            }
            Err(fault) => {
                self.trying = None;
                self.fault = Some(fault.to_string());
            }
        }
    }

    pub fn stop_trying(&mut self) {
        if let Some(v) = self.voicer.as_mut() {
            v.stop();
        }
        self.trying = None;
        self.tried = None;
    }

    /// Each frame while open: hears the sample end.
    pub fn tick(&mut self) {
        // A voice found mute, here or by the book's reading, leaves the list.
        if self
            .voicer
            .as_ref()
            .is_some_and(|v| v.voices_seen() != self.seen)
        {
            self.relist();
        }
        let (Some(v), Some(id)) = (self.voicer.as_mut(), self.trying) else {
            return;
        };
        if v.events().contains(&Event::Finished(id)) {
            self.trying = None;
            self.tried = None;
        } else {
            // Some systems only tell when asked.
            self.repaint.after(TICK);
        }
    }

    /// The engine open, or last tried.
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// Its voices, by language then name.
    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    pub fn caps(&self) -> Caps {
        self.caps
    }

    /// Why the engine won't open, or won't speak.
    pub fn fault(&self) -> Option<&str> {
        self.fault.as_deref()
    }

    /// True while the engine reads its model, before the sample sounds.
    pub fn loading(&self) -> bool {
        self.voicer.as_ref().is_some_and(|v| v.loading())
    }

    /// True while a sample is being said.
    pub fn trying(&self) -> bool {
        self.trying.is_some()
    }

    /// The voice being said, when asked for by name.
    pub fn trying_voice(&self) -> Option<&str> {
        self.trying.and(self.tried.as_deref())
    }

    /// Samples said since the window opened.
    pub fn samples(&self) -> u64 {
        self.said
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;
    use std::time::Duration;

    use mareader_speech::SILENT;

    use super::*;

    /// A window recording the waits asked of it, in order.
    fn recording() -> (Repaint, Arc<Mutex<Vec<Duration>>>) {
        let asks: Arc<Mutex<Vec<Duration>>> = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&asks);
        let repaint = Repaint::new(Arc::new(move |after| {
            if let Ok(mut log) = kept.lock() {
                log.push(after);
            }
        }));
        (repaint, asks)
    }

    /// The waits asked for; a poisoned lock reads as none asked.
    fn asked(asks: &Mutex<Vec<Duration>>) -> Vec<Duration> {
        asks.lock().map_or_else(|_| Vec::new(), |l| l.clone())
    }

    /// A booth holding a mute engine, so a sample starts and ends on demand.
    fn mute_booth() -> (Booth, Arc<Mutex<Vec<Duration>>>) {
        let (repaint, asks) = recording();
        let mut booth = Booth::default();
        booth.attach(repaint);
        let shelf = Shelf::default();
        booth.open(SILENT, &ReadAloudSettings::default(), &shelf);
        (booth, asks)
    }

    #[test]
    fn a_new_booth_is_closed_and_has_said_nothing() {
        let booth = Booth::default();
        assert!(!booth.is_open());
        assert!(!booth.ready());
        assert!(booth.voices().is_empty());
        assert!(booth.fault().is_none());
        assert!(!booth.loading());
        assert!(!booth.trying());
        assert!(booth.trying_voice().is_none());
        assert_eq!(booth.samples(), 0);
    }

    #[test]
    fn an_open_booth_lists_the_voices_by_language_then_name() {
        let (booth, asks) = mute_booth();
        assert!(booth.is_open());
        assert!(booth.ready(), "a mute engine still opens");
        assert_eq!(booth.engine(), SILENT);
        assert!(booth.fault().is_none());
        let ids: Vec<&str> = booth.voices().iter().map(|v| v.id.as_str()).collect();
        assert_eq!(ids, ["quiet-en", "hush-fr"], "en-US sorts before fr-FR");
        assert!(booth.caps().voices);
        assert_eq!(asked(&asks).len(), 0, "opening asks for no frame");
    }

    #[test]
    fn an_engine_that_is_not_there_leaves_a_fault() {
        let (repaint, asks) = recording();
        let mut booth = Booth::default();
        booth.attach(repaint);
        let shelf = Shelf::default();
        booth.open("no-such-engine", &ReadAloudSettings::default(), &shelf);
        assert!(booth.is_open(), "a fault still counts as tried");
        assert!(!booth.ready());
        assert!(booth.voices().is_empty());
        assert!(booth.fault().is_some());
        assert_eq!(asked(&asks).len(), 0);
    }

    #[test]
    fn a_sample_waiting_to_end_asks_for_a_frame_after_the_tick() {
        let (mut booth, asks) = mute_booth();
        let settings = ReadAloudSettings::default();
        booth.try_voice(&settings);
        assert!(booth.trying());
        assert_eq!(booth.samples(), 1);

        // The mute engine reports the start first, so the end is still owed.
        booth.tick();
        assert!(booth.trying(), "a start is not an end");
        assert_eq!(asked(&asks), vec![TICK], "it asks to be ticked again");

        // The second look hears the end, and then asks for nothing.
        booth.tick();
        assert!(!booth.trying());
        assert!(booth.trying_voice().is_none());
        assert_eq!(asked(&asks), vec![TICK], "one ask, not one per frame");
        assert_eq!(booth.samples(), 1, "the count outlives the sample");
    }

    #[test]
    fn a_close_lets_the_engine_go_but_keeps_the_window() {
        let (mut booth, asks) = mute_booth();
        booth.close();
        assert!(!booth.is_open());
        assert!(!booth.ready());
        assert!(booth.voices().is_empty());
        assert!(booth.fault().is_none());
        // The handle survives the wipe, so a reopened booth can still ask.
        booth.repaint.after(TICK);
        assert_eq!(asked(&asks), vec![TICK]);
    }

    #[test]
    fn a_booth_named_by_a_voice_it_has_not_listed_reopens_first() {
        let (mut booth, _) = mute_booth();
        let shelf = Shelf::default();
        let settings = ReadAloudSettings::default();
        booth.try_voice_of(&settings, &shelf, "quiet-en");
        assert_eq!(booth.trying_voice(), Some("quiet-en"));
        // A voice the engine has not listed reopens it, then says the sample.
        booth.try_voice_of(&settings, &shelf, "never-listed");
        assert!(booth.ready(), "the reopen left it able to speak");
        assert_eq!(booth.samples(), 2);
    }

    #[test]
    fn the_tone_is_the_settings_it_came_from() {
        let settings = ReadAloudSettings {
            rate: 1.5,
            pitch: 0.75,
            volume: 0.25,
            ..Default::default()
        };
        let tone = tone(&settings);
        assert_eq!(tone.rate, settings.rate);
        assert_eq!(tone.pitch, settings.pitch);
        assert_eq!(tone.volume, settings.volume);
    }
}
