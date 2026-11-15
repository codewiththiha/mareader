//! Loaded voice models and the speech they made, shared by every voicer.

use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use mareader_g2p::{Dialect, G2p};
use mareader_hanzi::Hanzi;

use crate::Usage;
use crate::timing::Mark;

/// Speech kept by default, in bytes.
const MEMORY: usize = 64 << 20;
/// How long an unused model stays loaded by default.
const IDLE: Duration = Duration::from_secs(120);

/// A source and its studio, with no voice loaded until one is asked for.
pub(crate) struct Engine<S, L> {
    pub source: S,
    pub studio: Studio,
    pub loaded: Option<L>,
}

impl<S, L> Engine<S, L> {
    pub(crate) fn new(source: S, studio: Studio) -> Self {
        Self {
            source,
            studio,
            loaded: None,
        }
    }
}

/// One loaded thing, kept only while someone holds it.
struct Slot<T>(Mutex<Weak<T>>);

impl<T> Default for Slot<T> {
    fn default() -> Self {
        Self(Mutex::new(Weak::new()))
    }
}

impl<T> Slot<T> {
    /// The loaded one, or `load`'s; one load at a time.
    fn get(&self, load: impl FnOnce() -> Result<T, String>) -> Result<Arc<T>, String> {
        let mut held = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(it) = held.upgrade() {
            return Ok(it);
        }
        let it = Arc::new(load()?);
        *held = Arc::downgrade(&it);
        Ok(it)
    }
}

/// What a sentence was said with, and the sentence.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    /// The model and build that said it, as `Kokoro (Small)`.
    pub model: Arc<str>,
    pub voice: String,
    /// The speed's bits, so equal speeds hash equal.
    pub speed: u32,
    /// The model's settings, as bits, in the order it lists them.
    pub knobs: Vec<u32>,
    pub text: String,
}

/// Mono samples, and how many play a second.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Pcm {
    pub samples: Arc<[i16]>,
    pub rate: u32,
    /// Where its words start, in order.
    pub marks: Arc<[Mark]>,
}

impl Pcm {
    pub fn new(samples: &[i16], rate: u32) -> Self {
        Self::marked(samples, rate, &[])
    }

    pub fn marked(samples: &[i16], rate: u32, marks: &[Mark]) -> Self {
        Self {
            samples: Arc::from(samples),
            rate,
            marks: Arc::from(marks),
        }
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn seconds(&self) -> f64 {
        self.len() as f64 / f64::from(self.rate.max(1))
    }
}

/// Speech already made, the least recently heard let go past the cap.
#[derive(Default)]
pub(crate) struct Takes {
    cap: usize,
    used: usize,
    clock: u64,
    kept: HashMap<Key, (Pcm, u64)>,
}

impl Takes {
    pub fn get(&mut self, key: &Key) -> Option<Pcm> {
        self.clock += 1;
        let (pcm, used) = self.kept.get_mut(key)?;
        *used = self.clock;
        Some(pcm.clone())
    }

    pub fn has(&self, key: &Key) -> bool {
        self.kept.contains_key(key)
    }

    pub fn put(&mut self, key: Key, pcm: Pcm) {
        self.clock += 1;
        self.used += bytes(&pcm);
        if let Some((old, _)) = self.kept.insert(key, (pcm, self.clock)) {
            self.used -= bytes(&old);
        }
        self.trim();
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.trim();
    }

    fn trim(&mut self) {
        while self.used > self.cap {
            let oldest = self.kept.iter().min_by_key(|(_, (_, used))| *used);
            let Some(key) = oldest.map(|(k, _)| k.clone()) else {
                return;
            };
            if let Some((pcm, _)) = self.kept.remove(&key) {
                self.used -= bytes(&pcm);
            }
        }
    }
}

fn bytes(pcm: &Pcm) -> usize {
    std::mem::size_of_val(&*pcm.samples)
}

/// A loaded model of any kind, by the name it shows as.
type Held = (Arc<str>, Weak<dyn Any + Send + Sync>);

/// Rung when the idle limit changes, so a waiting thread looks again.
pub(crate) trait Bell: Send + Sync {
    fn ring(&self);
}

#[derive(Default)]
struct Inner {
    /// Every model loaded, kept only while someone holds it.
    models: Mutex<Vec<Held>>,
    /// Held while a model loads, so each loads once.
    loading: Mutex<()>,
    american: Slot<G2p>,
    british: Slot<G2p>,
    hanzi: Slot<Hanzi>,
    takes: Mutex<Takes>,
    idle: Mutex<Option<Duration>>,
    /// Each engine's thread, told when the idle limit changes.
    bells: Mutex<Vec<Weak<dyn Bell>>>,
}

/// Models load once however many voicers use them, and go with the last.
#[derive(Clone)]
pub struct Studio(Arc<Inner>);

impl Default for Studio {
    fn default() -> Self {
        let studio = Self(Arc::default());
        studio.set_memory(MEMORY);
        studio.set_idle(Some(IDLE));
        studio
    }
}

impl Studio {
    /// Keeps at most `bytes` of speech made.
    pub fn set_memory(&self, bytes: usize) {
        self.takes().set_cap(bytes);
    }

    /// Lets an unused model go after `idle`; `None` keeps it while open.
    pub fn set_idle(&self, idle: Option<Duration>) {
        let mut held = self.0.idle.lock().unwrap_or_else(PoisonError::into_inner);
        let changed = *held != idle;
        *held = idle;
        // Let go first: a thread rung reads the limit again.
        drop(held);
        if changed {
            for bell in self.bells() {
                bell.ring();
            }
        }
    }

    /// Rings `bell` whenever the idle limit changes.
    pub(crate) fn hear(&self, bell: Weak<dyn Bell>) {
        let mut bells = self.0.bells.lock().unwrap_or_else(PoisonError::into_inner);
        bells.retain(|b| b.strong_count() > 0);
        bells.push(bell);
    }

    fn bells(&self) -> Vec<Arc<dyn Bell>> {
        let bells = self.0.bells.lock().unwrap_or_else(PoisonError::into_inner);
        bells.iter().filter_map(Weak::upgrade).collect()
    }

    pub(crate) fn idle(&self) -> Option<Duration> {
        *self.0.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn usage(&self) -> Usage {
        let models = self.models().iter().map(|(n, _)| n.to_string()).collect();
        Usage {
            models,
            speech: self.takes().used,
        }
    }

    /// Whether the model named `name` is loaded.
    pub(crate) fn has_model(&self, name: &str) -> bool {
        self.models().iter().any(|(n, _)| **n == *name)
    }

    /// The models still held, the rest let go of.
    fn models(&self) -> MutexGuard<'_, Vec<Held>> {
        let mut models = self.0.models.lock().unwrap_or_else(PoisonError::into_inner);
        models.retain(|(_, it)| it.strong_count() > 0);
        models
    }

    /// The model named `name`, loaded by `load` unless held already.
    pub(crate) fn model<T: Any + Send + Sync>(
        &self,
        name: &Arc<str>,
        load: impl FnOnce() -> Result<T, String>,
    ) -> Result<Arc<T>, String> {
        let find = |s: &Self| {
            let models = s.models();
            let held = models.iter().find(|(n, _)| n == name);
            held.and_then(|(_, it)| it.upgrade()?.downcast::<T>().ok())
        };
        let _one = self
            .0
            .loading
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(it) = find(self) {
            return Ok(it);
        }
        let it = Arc::new(load()?);
        let any: Arc<dyn Any + Send + Sync> = it.clone();
        let mut models = self.models();
        models.retain(|(n, _)| n != name);
        models.push((Arc::clone(name), Arc::downgrade(&any)));
        Ok(it)
    }

    pub(crate) fn takes(&self) -> MutexGuard<'_, Takes> {
        self.0.takes.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn g2p(
        &self,
        dialect: Dialect,
        load: impl FnOnce() -> Result<G2p, String>,
    ) -> Result<Arc<G2p>, String> {
        match dialect {
            Dialect::American => self.0.american.get(load),
            Dialect::British => self.0.british.get(load),
        }
    }

    /// The Chinese reader, loaded once for every Chinese voice.
    pub(crate) fn hanzi(
        &self,
        load: impl FnOnce() -> Result<Hanzi, String>,
    ) -> Result<Arc<Hanzi>, String> {
        self.0.hanzi.get(load)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        Key {
            model: "Kokoro (Full)".into(),
            voice: "af_heart".into(),
            speed: 1f32.to_bits(),
            knobs: Vec::new(),
            text: text.into(),
        }
    }

    fn pcm(samples: usize) -> Pcm {
        Pcm::new(&vec![0i16; samples], 24_000)
    }

    #[test]
    fn the_least_recently_heard_goes_first() {
        let mut takes = Takes::default();
        takes.set_cap(250);
        takes.put(key("a"), pcm(50));
        takes.put(key("b"), pcm(50));
        assert!(takes.get(&key("a")).is_some());
        takes.put(key("c"), pcm(50));
        assert!(takes.has(&key("a")) && takes.has(&key("c")));
        assert!(!takes.has(&key("b")), "b was heard least recently");
    }

    #[test]
    fn a_smaller_cap_lets_go_at_once_and_none_keeps_nothing() {
        let mut takes = Takes::default();
        takes.set_cap(1000);
        takes.put(key("a"), pcm(100));
        takes.put(key("a"), pcm(100));
        takes.put(key("b"), pcm(100));
        takes.set_cap(200);
        assert!(!takes.has(&key("a")) && takes.has(&key("b")));
        takes.set_cap(0);
        assert!(!takes.has(&key("b")));
        takes.put(key("c"), pcm(1));
        assert!(!takes.has(&key("c")));
    }

    #[test]
    fn usage_counts_speech_kept_and_no_model_until_one_loads() {
        let studio = Studio::default();
        assert_eq!(studio.usage(), Usage::default());
        studio.takes().put(key("a"), pcm(100));
        assert_eq!(studio.usage().speech, 200);
        assert_eq!(studio.idle(), Some(IDLE));
        studio.set_idle(None);
        assert_eq!(studio.idle(), None);
    }

    #[test]
    fn models_of_any_kind_load_once_show_by_name_and_go_with_the_last() {
        let studio = Studio::default();
        let (big, small): (Arc<str>, Arc<str>) = ("Big".into(), "Small".into());
        let one = studio.model(&big, || Ok(1u32)).unwrap();
        let again = studio.model(&big, || Ok(2u32)).unwrap();
        assert_eq!((*one, *again), (1, 1), "loaded once while held");
        let other = studio.model(&small, || Ok("words".to_owned())).unwrap();
        assert!(studio.has_model("Big") && studio.has_model("Small"));
        assert_eq!(studio.usage().models, ["Big", "Small"]);
        // The same name as another kind loads afresh.
        assert_eq!(*studio.model(&big, || Ok(3u64)).unwrap(), 3);
        drop((one, again));
        assert_eq!(studio.usage().models, ["Small"]);
        assert!(studio.model(&big, || Err::<u32, _>("gone".into())).is_err());
        drop(other);
        assert_eq!(studio.usage(), Usage::default());
        assert_eq!(pcm(48_000).seconds(), 2.0);
    }

    #[test]
    fn a_loaded_thing_is_shared_then_let_go() {
        let slot: Slot<u32> = Slot::default();
        let one = slot.get(|| Ok(1));
        let two = slot.get(|| Ok(2));
        assert_eq!(one.as_deref(), Ok(&1));
        assert_eq!(two.as_deref(), Ok(&1), "loaded once while held");
        let held = |s: &Slot<u32>| s.0.lock().unwrap().strong_count() > 0;
        assert!(held(&slot));
        drop((one, two));
        assert!(!held(&slot));
        assert_eq!(slot.get(|| Ok(3)).as_deref(), Ok(&3));
        assert!(slot.get(|| Err::<u32, _>("gone".into())).is_err());
    }
}
