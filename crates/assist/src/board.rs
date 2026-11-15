//! What the AI settings show, and the jobs they start. Plain data.

use std::collections::{HashMap, HashSet};

use mareader_settings::{ModelPreset, PresetId, Provider};

pub use mareader_settings::Job;
use mareader_study::Failure;

/// Something fetched in the background.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Load<T> {
    /// Not asked for yet.
    #[default]
    Idle,
    Busy,
    Ready(T),
    Failed(Failure),
}

impl<T> Load<T> {
    fn of(r: Result<T, Failure>) -> Self {
        r.map_or_else(Self::Failed, Self::Ready)
    }

    pub fn ready(&self) -> Option<&T> {
        match self {
            Self::Ready(t) => Some(t),
            _ => None,
        }
    }
}

/// Whether Apple's model can answer on this Mac.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppleState {
    /// Ready, in these BCP-47 languages.
    Ready(Vec<String>),
    /// Still downloading or preparing.
    NotReady(String),
    /// Switched off in System Settings.
    Off(String),
    /// Not on this Mac, or the helper is missing.
    Missing(String),
}

impl AppleState {
    /// Whether it writes `tag`, by language: `zh-Hans` finds `zh-CN`.
    pub fn speaks(&self, tag: &str) -> Option<bool> {
        let Self::Ready(languages) = self else {
            return None;
        };
        let primary = |t: &str| {
            t.split(['-', '_'])
                .next()
                .unwrap_or_default()
                .to_lowercase()
        };
        let wanted = primary(tag);
        Some(languages.iter().any(|l| primary(l) == wanted))
    }
}

/// A stored key's health.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyState {
    Ready,
    /// Rate limited; back by itself.
    Resting {
        secs: u64,
    },
    /// Turned away, set aside by hand, or undecryptable.
    Banned {
        reason: String,
    },
}

/// One key, as it may be shown: never the key itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyRow {
    pub id: String,
    /// The start and end, like `sk-proj…8f92`.
    pub shown: String,
    pub state: KeyState,
}

/// Keys a task may rotate over at once: all but banned ones, at least one.
pub fn lanes(keys: &[KeyRow]) -> u32 {
    let usable = keys
        .iter()
        .filter(|k| !matches!(k.state, KeyState::Banned { .. }))
        .count();
    u32::try_from(usable).unwrap_or(u32::MAX).max(1)
}

/// A trial answer from the settings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tried {
    pub millis: u64,
    pub meaning: String,
}

/// A finished job.
#[derive(Clone, Debug, PartialEq)]
pub enum Reply {
    Apple(AppleState),
    /// The keys after a change; `problem` if the change itself failed.
    Keys {
        provider: Provider,
        keys: Result<Vec<KeyRow>, Failure>,
        problem: Option<Failure>,
    },
    Models(Provider, Result<Vec<String>, Failure>),
    Tried(PresetId, Result<Tried, Failure>),
}

/// Everything the AI settings show that isn't a setting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Board {
    pub apple: Load<AppleState>,
    pub keys: HashMap<Provider, Load<Vec<KeyRow>>>,
    /// The last key change that failed, per provider.
    pub problems: HashMap<Provider, Failure>,
    pub models: HashMap<Provider, Load<Vec<String>>>,
    /// Lists fetched again; the old one stays shown meanwhile.
    pub refreshing: HashSet<Provider>,
    pub tried: HashMap<PresetId, Load<Tried>>,
}

impl Board {
    pub fn keys(&self, p: Provider) -> &Load<Vec<KeyRow>> {
        self.keys.get(&p).unwrap_or(&Load::Idle)
    }

    pub fn models(&self, p: Provider) -> &Load<Vec<String>> {
        self.models.get(&p).unwrap_or(&Load::Idle)
    }

    /// Whether `p`'s models are on their way, with or without a list.
    pub fn fetching(&self, p: Provider) -> bool {
        self.refreshing.contains(&p) || matches!(self.models(p), Load::Busy)
    }

    pub fn tried(&self, id: PresetId) -> &Load<Tried> {
        self.tried.get(&id).unwrap_or(&Load::Idle)
    }

    /// Tasks `p` may run at once: `at_once` per usable key, if keys are known.
    pub fn capacity(&self, p: &ModelPreset) -> u32 {
        let keys = match p.provider.takes_keys() {
            true => self.keys(p.provider).ready().map_or(1, |k| lanes(k)),
            false => 1,
        };
        u32::from(p.at_once).saturating_mul(keys)
    }

    /// Marks what `job` will refresh as on its way.
    pub fn start(&mut self, job: &Job) {
        match job {
            Job::Probe => self.apple = Load::Busy,
            Job::Keys(p)
            | Job::AddKey(p, _)
            | Job::RemoveKey(p, _)
            | Job::BanKey(p, _)
            | Job::RecoverKey(p, _) => {
                // Rows stay on screen while a change is made.
                self.keys.entry(*p).or_insert(Load::Busy);
                self.problems.remove(p);
            }
            Job::Models(p) => match self.models(*p) {
                Load::Ready(_) => {
                    self.refreshing.insert(*p);
                }
                _ => {
                    self.models.insert(*p, Load::Busy);
                }
            },
            Job::Try(p) => {
                self.tried.insert(p.id, Load::Busy);
            }
        }
    }

    pub fn finish(&mut self, reply: Reply) {
        match reply {
            Reply::Apple(state) => self.apple = Load::Ready(state),
            Reply::Keys {
                provider,
                keys,
                problem,
            } => {
                self.keys.insert(provider, Load::of(keys));
                if let Some(p) = problem {
                    self.problems.insert(provider, p);
                }
            }
            Reply::Models(p, models) => {
                self.refreshing.remove(&p);
                self.models.insert(p, Load::of(models));
            }
            Reply::Tried(p, tried) => {
                self.tried.insert(p, Load::of(tried));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use mareader_study::Why;

    use super::*;

    #[test]
    fn a_refresh_keeps_the_old_list_until_the_new_one() {
        let mut b = Board::default();
        let p = Provider::Ollama;
        b.start(&Job::Models(p));
        assert!(b.fetching(p));
        let old = vec!["llama3.2:latest".to_owned()];
        b.finish(Reply::Models(p, Ok(old.clone())));
        assert!(!b.fetching(p));
        b.start(&Job::Models(p));
        assert!(b.fetching(p));
        assert_eq!(b.models(p), &Load::Ready(old));
        let new = vec!["qwen3:4b".to_owned()];
        b.finish(Reply::Models(p, Ok(new.clone())));
        assert!(!b.fetching(p));
        assert_eq!(b.models(p), &Load::Ready(new));
    }

    #[test]
    fn jobs_mark_what_they_refresh_and_replies_land() {
        let mut b = Board::default();
        let p = Provider::OpenAi;
        b.start(&Job::Keys(p));
        assert_eq!(b.keys(p), &Load::Busy);
        let row = KeyRow {
            id: "k1".into(),
            shown: "sk-…1234".into(),
            state: KeyState::Ready,
        };
        let problem = Some(Failure::from(Why::Setup));
        let keys = Ok(vec![row.clone()]);
        b.finish(Reply::Keys {
            provider: p,
            keys,
            problem,
        });
        assert_eq!(b.keys(p).ready().map(Vec::len), Some(1));
        assert!(b.problems.contains_key(&p));
        b.start(&Job::RemoveKey(p, "k1".into()));
        assert_eq!(b.keys(p).ready(), Some(&vec![row]), "rows stay meanwhile");
        assert!(!b.problems.contains_key(&p));
        assert_eq!(b.models(Provider::Gemini), &Load::Idle);
    }

    #[test]
    fn capacity_is_at_once_per_usable_key() {
        let mut b = Board::default();
        let mut ai = mareader_settings::AiSettings::default();
        let id = ai.add_preset(Provider::OpenAi, "");
        let mut p = ai.preset(id).cloned().unwrap_or_default();
        p.at_once = 2;
        assert_eq!(b.capacity(&p), 2, "keys not listed yet count as one");
        let key = |id: &str, state| KeyRow {
            id: id.into(),
            shown: String::new(),
            state,
        };
        let banned = KeyState::Banned {
            reason: String::new(),
        };
        let rows = vec![
            key("a", KeyState::Ready),
            key("b", KeyState::Resting { secs: 3 }),
            key("c", banned),
        ];
        b.finish(Reply::Keys {
            provider: Provider::OpenAi,
            keys: Ok(rows),
            problem: None,
        });
        assert_eq!(b.capacity(&p), 4, "resting keys come back; banned don't");
        p.provider = Provider::Ollama;
        assert_eq!(b.capacity(&p), 2, "a keyless server is one lane");
        assert_eq!(lanes(&[]), 1);
    }

    #[test]
    fn apple_speaks_by_language_not_region() {
        let ready = AppleState::Ready(vec!["en-US".into(), "zh-CN".into()]);
        assert_eq!(ready.speaks("en"), Some(true));
        assert_eq!(ready.speaks("zh-Hans"), Some(true));
        assert_eq!(ready.speaks("my"), Some(false));
        assert_eq!(AppleState::Off(String::new()).speaks("en"), None);
    }

    #[test]
    fn a_job_never_shows_its_key() {
        let job = Job::AddKey(Provider::Gemini, "sk-secret".into());
        assert!(!format!("{job:?}").contains("sk-secret"));
    }
}
