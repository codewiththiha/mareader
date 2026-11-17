//! System voices via `tts`: kept on the window's thread, never asked its voice.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::{Duration, Instant};

use crate::{Caps, Event, Fault, Tone, Voice, Voicer, Waker, scaled};

/// The speeds and pitches the reader offers, as shares of normal.
const RATES: (f32, f32) = (0.5, 3.0);
const PITCHES: (f32, f32) = (0.5, 2.0);
/// Characters a second at normal speed, when nothing reports an end.
const PACE: f32 = 14.0;

/// Voices reporting their sound unspoken: past this, the reports are true.
const WARM: usize = 10;

/// Apple's speech opens once a run: a second open fails, so voicers share it.
const ONCE: bool = cfg!(target_vendor = "apple");

thread_local! {
    /// Shared speech when opened once, else one idle handle parked for reuse.
    static SHARED: RefCell<Option<Rc<Speech>>> = const { RefCell::new(None) };
}

/// Numbers voicers, so a shared speech knows who spoke and who tuned it.
static NEXT: AtomicU64 = AtomicU64::new(1);

/// What one voicer heard begin and end, by the engine's name for it.
struct Ear {
    heard: Mutex<Vec<(bool, String)>>,
    wake: Waker,
}

/// Every voicer's ear on one speech; each picks out its own utterances.
type Ears = Arc<Mutex<Vec<Weak<Ear>>>>;

/// The system's speech and what it offers, as one or more voicers share it.
struct Speech {
    tts: tts::Tts,
    features: tts::Features,
    /// Its voices, read once: some systems are slow to list them.
    voices: Vec<tts::Voice>,
    ears: Ears,
    /// The voicer whose voice and tone it has, and the one that spoke last.
    tuned: Cell<u64>,
    speaker: Cell<u64>,
    /// A voice was set, so the default must be found again.
    voiced: Cell<bool>,
    /// Listed voices with no sound here, which the system reads in another.
    mute: RefCell<HashSet<String>>,
    /// Voices heard to the end, and whether `mute` is known for good.
    spoken: RefCell<HashSet<String>>,
    settled: Cell<bool>,
    /// Counts changes to `mute`.
    seen: Cell<u64>,
}

impl Speech {
    fn open() -> Result<Self, tts::Error> {
        let tts = tts::Tts::default()?;
        let mut features = tts.supported_features();
        let voices = match features.voice {
            true => tts.voices().unwrap_or_default(),
            false => Vec::new(),
        };
        let ears: Ears = Arc::default();
        if features.utterance_callbacks {
            let hear = |begun: bool| {
                let ears = Arc::clone(&ears);
                Box::new(move |id: tts::UtteranceId| {
                    let name = format!("{id:?}");
                    let ears = ears.lock().unwrap_or_else(PoisonError::into_inner);
                    for ear in ears.iter().filter_map(Weak::upgrade) {
                        let mut heard = ear.heard.lock().unwrap_or_else(PoisonError::into_inner);
                        heard.push((begun, name.clone()));
                        drop(heard);
                        (ear.wake)();
                    }
                }) as Box<dyn FnMut(tts::UtteranceId)>
            };
            // An engine that refuses callbacks falls back to watching.
            let begin = tts.on_utterance_begin(Some(hear(true)));
            let end = tts.on_utterance_end(Some(hear(false)));
            if begin.is_err() || end.is_err() {
                features.utterance_callbacks = false;
            }
        }
        let speech = Self {
            tts,
            features,
            voices,
            ears,
            tuned: Cell::new(0),
            speaker: Cell::new(0),
            voiced: Cell::new(false),
            mute: RefCell::default(),
            spoken: RefCell::default(),
            settled: Cell::new(false),
            seen: Cell::new(0),
        };
        speech.judge();
        Ok(speech)
    }

    /// Learns again which voices are mute, until that is known for good.
    fn judge(&self) {
        if self.settled.get() {
            return;
        }
        let Some(sounds) = sounds() else {
            self.settled.set(true);
            return;
        };
        let (mute, settled) = mute(&sounds, &self.spoken.borrow());
        if mute != *self.mute.borrow() {
            *self.mute.borrow_mut() = mute;
            self.seen.set(self.seen.get() + 1);
        }
        self.settled.set(settled);
    }

    /// This thread's speech: shared where it opens once, else reused when idle.
    fn get() -> Result<Rc<Self>, tts::Error> {
        SHARED.with(|shared| {
            let hit = match ONCE {
                true => shared.borrow().as_ref().map(Rc::clone),
                false => shared.borrow_mut().take(),
            };
            if let Some(speech) = hit {
                return Ok(speech);
            }
            // Only an open that worked is kept; a failed one is tried again.
            let speech = Rc::new(Self::open()?);
            if ONCE {
                *shared.borrow_mut() = Some(Rc::clone(&speech));
            }
            Ok(speech)
        })
    }

    /// Lets `ear` hear its utterances, forgetting voicers gone.
    fn listen(&self, ear: &Arc<Ear>) {
        let mut ears = self.ears.lock().unwrap_or_else(PoisonError::into_inner);
        ears.retain(|e| e.strong_count() > 0);
        ears.push(Arc::downgrade(ear));
    }
}

pub struct System {
    speech: Rc<Speech>,
    /// Its own handle on the speech, which setting needs.
    tts: tts::Tts,
    me: u64,
    ear: Arc<Ear>,
    /// The engine's name for what is being said, and ours.
    current: Option<(String, u64)>,
    said_at: Instant,
    /// How long what is being said should take, when nothing tells.
    expect: Duration,
    voice: Option<String>,
    tone: Tone,
}

impl System {
    pub fn new(wake: Waker) -> Result<Self, Fault> {
        let speech = Speech::get()
            .map_err(|e| Fault::Unavailable(format!("System voices aren't available: {e}")))?;
        let ear = Arc::new(Ear {
            heard: Mutex::new(Vec::new()),
            wake,
        });
        speech.listen(&ear);
        Ok(Self {
            tts: speech.tts.clone(),
            speech,
            me: NEXT.fetch_add(1, Ordering::Relaxed),
            ear,
            current: None,
            said_at: Instant::now(),
            expect: Duration::ZERO,
            voice: None,
            tone: Tone::default(),
        })
    }

    fn features(&self) -> &tts::Features {
        &self.speech.features
    }

    /// Gives the speech this voicer's voice and tone, once another had it.
    fn tune(&mut self) {
        let speech = Rc::clone(&self.speech);
        let picked = self.voice.as_deref();
        match picked.and_then(|id| speech.voices.iter().find(|v| v.id() == id)) {
            Some(v) => {
                if self.tts.set_voice(v).is_ok() {
                    speech.voiced.set(true);
                }
            }
            None if speech.voiced.get() => self.unvoice(),
            None => {}
        }
        let tone = self.tone;
        self.apply(tone);
        self.speech.tuned.set(self.me);
    }

    /// Back to the system's default voice, which no call names.
    fn unvoice(&mut self) {
        if ONCE {
            // Apple's speech can't open again: its usual voice stands in.
            let voices = &self.speech.voices;
            let pairs = voices.iter().map(|v| (v.id(), v.language().to_string()));
            let pairs: Vec<_> = pairs.collect();
            let mine = locale().and_then(|l| usual(pairs.iter().cloned(), &l));
            // Else the last voicer's voice would stay on.
            let usual = mine.or_else(|| usual(pairs.into_iter(), "en-US"));
            if let Some(v) = usual.and_then(|i| voices.get(i)) {
                let _ = self.tts.set_voice(v);
            }
            return;
        }
        // Only a fresh start finds the default again.
        let Ok(speech) = Speech::open() else {
            return;
        };
        speech.listen(&self.ear);
        self.tts = speech.tts.clone();
        self.speech = Rc::new(speech);
    }

    /// Sets what the system offers of `tone`; the rest is left as it is.
    fn apply(&mut self, tone: Tone) {
        let features = self.features();
        let (rate, pitch, volume) = (features.rate, features.pitch, features.volume);
        if rate {
            let _ = self.tts.set_rate(self.rate(tone.rate));
        }
        if pitch {
            let range = (
                self.tts.min_pitch(),
                self.tts.normal_pitch(),
                self.tts.max_pitch(),
            );
            let _ = self.tts.set_pitch(scaled(tone.pitch, range, PITCHES));
        }
        if volume {
            let (min, max) = (self.tts.min_volume(), self.tts.max_volume());
            let _ = self
                .tts
                .set_volume(min + (max - min) * tone.volume.clamp(0.0, 1.0));
        }
    }

    fn rate(&self, share: f32) -> f32 {
        let (min, normal, mut max) = (
            self.tts.min_rate(),
            self.tts.normal_rate(),
            self.tts.max_rate(),
        );
        // Windows takes a plain multiple of normal speed.
        if cfg!(windows) {
            return share.clamp(min, max);
        }
        // Past 1 the Mac's synthesizer goes no faster.
        if cfg!(target_os = "macos") {
            max = max.min(1.0);
        }
        scaled(share, (min, normal, max), RATES)
    }

    /// The speech says what this voicer said last, not another's.
    fn speaking(&self) -> bool {
        self.speech.speaker.get() == self.me
    }
}

/// The user's language tag, as `en-US`.
#[cfg(target_vendor = "apple")]
pub(crate) fn locale() -> Option<String> {
    sys_locale::get_locale()
}

#[cfg(not(target_vendor = "apple"))]
pub(crate) fn locale() -> Option<String> {
    None
}

/// Which of `voices` (id, language) the system would pick for `locale`.
fn usual(voices: impl Iterator<Item = (String, String)>, locale: &str) -> Option<usize> {
    let locale = locale.replace('_', "-");
    let language = locale.split('-').next().unwrap_or_default().to_owned();
    // Novelty voices share it: compact ones, like Samantha, come first.
    let rank = |(id, tag): &(String, String)| {
        let same = tag.eq_ignore_ascii_case(&locale);
        let kin = tag.split('-').next().unwrap_or_default();
        let kin = kin.eq_ignore_ascii_case(&language);
        (!kin, !same, !id.contains(".voice.compact."))
    };
    let (i, best) = voices.enumerate().min_by_key(|(_, v)| rank(v))?;
    (!rank(&best).0).then_some(i)
}

/// Each listed voice's id and how many audio settings it reports.
#[cfg(target_vendor = "apple")]
fn sounds() -> Option<Vec<(String, usize)>> {
    Some(crate::mac_voices::sounds())
}

/// Elsewhere every listed voice has its sound.
#[cfg(not(target_vendor = "apple"))]
fn sounds() -> Option<Vec<(String, usize)>> {
    None
}

/// The mute voices among `sounds`, and whether that is known for good.
fn mute(sounds: &[(String, usize)], spoken: &HashSet<String>) -> (HashSet<String>, bool) {
    // Until speech starts, macOS reports no settings for any voice.
    let alive = sounds.iter().any(|(_, n)| *n > 0);
    let unspoken = sounds
        .iter()
        .filter(|(id, n)| *n > 0 && !spoken.contains(id));
    let warm = unspoken.count() >= WARM;
    // Cold, only a voice heard to the end and still bare is known mute.
    let found: HashSet<String> = (sounds.iter())
        .filter(|(id, n)| *n == 0 && (warm || (alive && spoken.contains(id))))
        .map(|(id, _)| id.clone())
        .collect();
    // Every voice mute would be a report gone wrong.
    if found.len() >= sounds.len() {
        return (HashSet::new(), false);
    }
    (found, warm)
}

impl Voicer for System {
    fn caps(&self) -> Caps {
        let features = self.features();
        Caps {
            rate: features.rate,
            pitch: features.pitch,
            volume: features.volume,
            voices: features.voice && !self.speech.voices.is_empty(),
            pause: false,
        }
    }

    fn voices(&self) -> Vec<Voice> {
        let voice = |v: &tts::Voice| Voice {
            id: v.id(),
            name: v.name(),
            language: v.language().to_string(),
        };
        let mute = self.speech.mute.borrow();
        let heard = self
            .speech
            .voices
            .iter()
            .filter(|v| !mute.contains(&v.id()));
        heard.map(voice).collect()
    }

    fn voices_seen(&self) -> u64 {
        self.speech.seen.get()
    }

    fn set_voice(&mut self, id: Option<&str>) {
        if self.voice.as_deref() == id || !self.features().voice {
            return;
        }
        let known = id.filter(|id| self.speech.voices.iter().any(|v| v.id() == *id));
        self.voice = known.map(str::to_owned);
        self.tune();
    }

    fn set_tone(&mut self, tone: Tone) {
        self.tone = tone;
        self.tune();
    }

    fn say(&mut self, id: u64, text: &str) -> Result<(), Fault> {
        // A voicer sharing the speech may have tuned it since.
        if self.speech.tuned.get() != self.me {
            self.tune();
        }
        let said = self.tts.speak(text, true);
        let said = said.map_err(|e| Fault::Failed(format!("The system voice failed: {e}")))?;
        self.speech.speaker.set(self.me);
        let name = said.map(|u| format!("{u:?}")).unwrap_or_default();
        self.current = Some((name, id));
        self.said_at = Instant::now();
        let chars = text.chars().count() as f32;
        let secs = chars / (PACE * self.tone.rate.max(0.1));
        self.expect = Duration::from_secs_f32(secs.clamp(0.3, 120.0));
        Ok(())
    }

    fn stop(&mut self) {
        self.current = None;
        // What another voicer says is not this one's to stop.
        if self.speaking() {
            let _ = self.tts.stop();
        }
    }

    fn events(&mut self) -> Vec<Event> {
        let heard = {
            let mut heard = self
                .ear
                .heard
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            std::mem::take(&mut *heard)
        };
        // Another voicer spoke over this one: what it said was cut off.
        if !self.speaking() {
            self.current = None;
        }
        let mut out = Vec::new();
        let mut ended = false;
        for (begun, name) in heard {
            let Some((ours, id)) = self.current.clone() else {
                continue;
            };
            // A late word about something already cut off is dropped.
            if name != ours {
                continue;
            }
            match begun {
                true => out.push(Event::Started(id)),
                false => {
                    out.push(Event::Finished(id));
                    self.current = None;
                    ended = true;
                }
            }
        }
        if ended {
            if let Some(voice) = self.voice.clone() {
                self.speech.spoken.borrow_mut().insert(voice);
            }
            self.speech.judge();
        }
        if !self.features().utterance_callbacks
            && let Some((_, id)) = self.current.clone()
        {
            // Some time to start, then silence means it ended.
            let elapsed = self.said_at.elapsed();
            let quiet = match self.features().is_speaking {
                true => {
                    elapsed > Duration::from_millis(400)
                        && matches!(self.tts.is_speaking(), Ok(false))
                }
                false => elapsed > self.expect,
            };
            if quiet {
                out.push(Event::Finished(id));
                self.current = None;
            }
        }
        out
    }
}

impl Drop for System {
    fn drop(&mut self) {
        if ONCE || self.current.is_some() || Rc::strong_count(&self.speech) != 1 {
            return;
        }
        self.speech.tuned.set(0);
        self.speech.speaker.set(0);
        let speech = Rc::clone(&self.speech);
        let _ = SHARED.try_with(|s| {
            let mut slot = s.borrow_mut();
            if slot.is_none() {
                *slot = Some(speech);
            }
        });
    }
}

#[cfg(test)]
#[path = "system_tests.rs"]
mod tests;
