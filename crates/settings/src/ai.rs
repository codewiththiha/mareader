//! AI: named models, which task uses which, and each server's address.

use std::collections::BTreeMap;

use mareader_dict::LANGUAGES;
use mareader_study::mode::{PER_REQUEST, PER_REQUEST_MAX};
use serde::{Deserialize, Serialize};

use crate::ai_task::{Class, Routes, TaskId, known, prune};
use crate::study_mode::drop_unknown;

/// Who answers. Keys are not here: they live encrypted, apart.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    /// Apple's on-device model, through the bundled helper.
    #[default]
    Apple,
    #[serde(rename = "openai")]
    OpenAi,
    Gemini,
    #[serde(rename = "openrouter")]
    OpenRouter,
    /// A local Ollama server; no key.
    Ollama,
    /// A local LM Studio server; no key.
    LmStudio,
    /// Any OpenAI-compatible server; a key only if it has some.
    Custom,
}

impl Provider {
    pub const ALL: [Provider; 7] = [
        Self::Apple,
        Self::OpenAi,
        Self::Gemini,
        Self::OpenRouter,
        Self::Ollama,
        Self::LmStudio,
        Self::Custom,
    ];

    /// The ones reached over HTTP, each with its own address and keys.
    pub const SERVERS: [Provider; 6] = [
        Self::OpenAi,
        Self::Gemini,
        Self::OpenRouter,
        Self::Ollama,
        Self::LmStudio,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Apple => "Apple Intelligence",
            Self::OpenAi => "OpenAI",
            Self::Gemini => "Google Gemini",
            Self::OpenRouter => "OpenRouter",
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::Custom => "Custom server",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            Self::Apple => "On this Mac, private and offline",
            Self::OpenAi => "Needs an API key",
            Self::Gemini => "Needs an API key",
            Self::OpenRouter => "Many models, one API key",
            Self::Ollama => "A model running on this computer",
            Self::LmStudio => "A model running on this computer",
            Self::Custom => "Any OpenAI-compatible address",
        }
    }

    /// The address when none is set; a custom server has none.
    pub fn default_url(self) -> &'static str {
        match self {
            Self::OpenAi => "https://api.openai.com/v1/",
            Self::Gemini => "https://generativelanguage.googleapis.com/v1beta/openai/",
            Self::OpenRouter => "https://openrouter.ai/api/v1/",
            Self::Ollama => "http://localhost:11434/v1/",
            Self::LmStudio => "http://localhost:1234/v1/",
            Self::Apple | Self::Custom => "",
        }
    }

    /// The model when none is set; empty asks the server for its first.
    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAi => "gpt-4.1-mini",
            Self::Gemini => "gemini-2.5-flash",
            Self::OpenRouter => "openai/gpt-4.1-mini",
            _ => "",
        }
    }

    /// Whether requests need a key; a custom server needs one if it has one.
    pub fn needs_key(self) -> bool {
        matches!(self, Self::OpenAi | Self::Gemini | Self::OpenRouter)
    }

    /// Whether it may hold keys at all.
    pub fn takes_keys(self) -> bool {
        self.needs_key() || self == Self::Custom
    }

    /// Whether it is a server on this computer.
    pub fn local(self) -> bool {
        matches!(self, Self::Ollama | Self::LmStudio)
    }

    /// Whether it runs over HTTP, with an address.
    pub fn server(self) -> bool {
        self != Self::Apple
    }

    /// Its keys' scope in the key store; stable, never shown.
    pub fn scope(self) -> &'static str {
        match self {
            Self::Apple => "apple",
            Self::OpenAi => "openai",
            Self::Gemini => "gemini",
            Self::OpenRouter => "openrouter",
            Self::Ollama => "ollama",
            Self::LmStudio => "lm_studio",
            Self::Custom => "custom",
        }
    }
}

/// Where one server is; blank means its default.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Endpoint {
    pub url: String,
}

/// Longer text is cut; a pasted page can't bloat the settings file.
pub const TEXT_MAX: usize = 2_048;

pub(crate) fn cut(s: &mut String, max: usize) {
    if let Some((byte, _)) = s.char_indices().nth(max) {
        s.truncate(byte);
    }
}

/// A saved model's handle; tasks point at it.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct PresetId(pub u32);

/// A model the reader named, ready for any task.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelPreset {
    pub id: PresetId,
    pub name: String,
    pub provider: Provider,
    /// Blank means the provider's default, or the server's first.
    pub model: String,
    /// Tasks at once on each usable key; keyless, in all.
    pub at_once: u8,
    /// Study-card words one request asks this model; a deck with more is
    /// cut into several (`mareader_study::mode::chunks`).
    pub cards_per_request: u8,
}

impl Default for ModelPreset {
    fn default() -> Self {
        Self {
            id: PresetId::default(),
            name: String::new(),
            provider: Provider::default(),
            model: String::new(),
            at_once: 1,
            cards_per_request: PER_REQUEST,
        }
    }
}

impl ModelPreset {
    pub const AT_ONCE_MAX: u8 = 8;
    pub const NAME_MAX: usize = 60;

    /// Its name, or what it runs while the name is blank.
    pub fn title(&self) -> String {
        match self.name.trim() {
            "" => Self::suggested_name(self.provider, &self.model),
            name => name.to_owned(),
        }
    }

    /// Its model: the one set, else the provider's default.
    pub fn model(&self) -> &str {
        match self.model.trim() {
            "" => self.provider.default_model(),
            set => set,
        }
    }

    /// The name a new preset starts with, from what it runs.
    pub fn suggested_name(provider: Provider, model: &str) -> String {
        match model.trim() {
            "" => provider.label().to_owned(),
            model => format!("{} · {model}", provider.label()),
        }
    }

    fn sanitize(&mut self) {
        cut(&mut self.name, Self::NAME_MAX);
        cut(&mut self.model, TEXT_MAX);
        self.at_once = self.at_once.clamp(1, Self::AT_ONCE_MAX);
        self.cards_per_request = self.cards_per_request.clamp(1, PER_REQUEST_MAX);
    }
}

/// Explaining a word: the meaning's language.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExplainTask {
    /// A tag of `mareader_dict::LANGUAGES`; none: the book's language.
    pub language: Option<String>,
}

/// Filing loose books onto shelves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OrganizeTask {
    /// Books the model reads per request.
    pub batch: u16,
}

impl OrganizeTask {
    pub const BATCH_MIN: u16 = 10;
    pub const BATCH_MAX: u16 = 200;
    pub const BATCH: u16 = 50;
}

impl Default for OrganizeTask {
    fn default() -> Self {
        Self {
            batch: OrganizeTask::BATCH,
        }
    }
}

/// How a page summary reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryStyle {
    #[default]
    Paragraphs,
    Points,
}

/// Summarizing a page.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SummarizeTask {
    pub style: SummaryStyle,
}

/// What choosing Translate or Simplify does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageStart {
    /// Offers This page and Continuous.
    #[default]
    Ask,
    Page,
    Continuous,
}

/// Translating pages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TranslateTask {
    /// A tag of `mareader_dict::LANGUAGES`; none until one is picked.
    pub language: Option<String>,
    /// Tags picked before, latest first.
    pub recent: Vec<String>,
    pub start: PageStart,
}

impl TranslateTask {
    pub const RECENT_MAX: usize = 5;

    /// Picks `tag` and puts it first among the recent ones.
    pub fn pick(&mut self, tag: &str) {
        if language_name(tag).is_none() {
            return;
        }
        self.language = Some(tag.to_owned());
        self.recent.retain(|t| t != tag);
        self.recent.insert(0, tag.to_owned());
        self.recent.truncate(Self::RECENT_MAX);
    }
}

/// Rewriting pages in simple English.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SimplifyTask {
    pub start: PageStart,
}

/// How a continuous run reads ahead.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageRun {
    /// Seconds on a page with its result before the next pages are asked.
    pub dwell: u16,
    /// Pages asked at once, at most; the model's room may allow fewer.
    pub at_once: u8,
    /// Picked pages sent in one request, at most.
    pub per_request: u8,
}

impl PageRun {
    pub const DWELL_MIN: u16 = 5;
    pub const DWELL_MAX: u16 = 120;
    pub const DWELL: u16 = 25;
    pub const AT_ONCE_MAX: u8 = 16;
    pub const AT_ONCE: u8 = 5;
    pub const PER_REQUEST_MAX: u8 = 20;
    pub const PER_REQUEST: u8 = 5;
}

impl Default for PageRun {
    fn default() -> Self {
        Self {
            dwell: Self::DWELL,
            at_once: Self::AT_ONCE,
            per_request: Self::PER_REQUEST,
        }
    }
}

/// Which models each AI task asks, and each one's own options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiTasks {
    /// Each task's models, first choice first; see [`AiSettings::route`].
    #[serde(deserialize_with = "known")]
    pub models: Routes,
    /// The reader's class per task; a task not here keeps its row's.
    #[serde(deserialize_with = "known")]
    pub priority: BTreeMap<TaskId, Class>,
    /// Models every task tries after its own; see [`AiSettings::backups`].
    pub backups: crate::Backups,
    pub explain: ExplainTask,
    pub organize: OrganizeTask,
    pub summarize: SummarizeTask,
    pub translate: TranslateTask,
    pub simplify: SimplifyTask,
    pub pages: PageRun,
    pub study: crate::StudyTask,
    /// The skills' own models, and the reader's skills.
    pub skills: crate::Skills,
}

/// The AI tasks that rewrite a page.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageTask {
    Summarize,
    Translate,
    Simplify,
}

impl PageTask {
    pub const ALL: [PageTask; 3] = [Self::Summarize, Self::Translate, Self::Simplify];

    pub fn label(self) -> &'static str {
        match self {
            Self::Summarize => "Summarize",
            Self::Translate => "Translate",
            Self::Simplify => "Simplify",
        }
    }

    /// Its name in commands and the settings file.
    pub fn key(self) -> &'static str {
        match self {
            Self::Summarize => "summarize",
            Self::Translate => "translate",
            Self::Simplify => "simplify",
        }
    }
}

/// The English name of a language tag, if listed.
pub fn language_name(tag: &str) -> Option<&'static str> {
    LANGUAGES.iter().find(|l| l.tag == tag).map(|l| l.name)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AiSettings {
    pub presets: Vec<ModelPreset>,
    pub tasks: AiTasks,
    pub openai: Endpoint,
    pub gemini: Endpoint,
    pub openrouter: Endpoint,
    pub ollama: Endpoint,
    pub lm_studio: Endpoint,
    pub custom: Endpoint,
}

impl Default for AiSettings {
    /// A Mac starts with Apple Intelligence explaining; others with none.
    fn default() -> Self {
        let mut ai = Self {
            presets: Vec::new(),
            tasks: AiTasks::default(),
            openai: Endpoint::default(),
            gemini: Endpoint::default(),
            openrouter: Endpoint::default(),
            ollama: Endpoint::default(),
            lm_studio: Endpoint::default(),
            custom: Endpoint::default(),
        };
        if cfg!(target_os = "macos") {
            let id = ai.add_preset(Provider::Apple, "");
            ai.tasks.choose(TaskId::Explain, Some(id));
        }
        ai
    }
}

impl AiSettings {
    pub const PRESETS_MAX: usize = 32;

    pub fn endpoint(&self, p: Provider) -> Option<&Endpoint> {
        match p {
            Provider::OpenAi => Some(&self.openai),
            Provider::Gemini => Some(&self.gemini),
            Provider::OpenRouter => Some(&self.openrouter),
            Provider::Ollama => Some(&self.ollama),
            Provider::LmStudio => Some(&self.lm_studio),
            Provider::Custom => Some(&self.custom),
            Provider::Apple => None,
        }
    }

    pub fn endpoint_mut(&mut self, p: Provider) -> Option<&mut Endpoint> {
        match p {
            Provider::OpenAi => Some(&mut self.openai),
            Provider::Gemini => Some(&mut self.gemini),
            Provider::OpenRouter => Some(&mut self.openrouter),
            Provider::Ollama => Some(&mut self.ollama),
            Provider::LmStudio => Some(&mut self.lm_studio),
            Provider::Custom => Some(&mut self.custom),
            Provider::Apple => None,
        }
    }

    /// `p`'s address: the one set, else its default; empty if neither.
    pub fn url(&self, p: Provider) -> String {
        let set = self.endpoint(p).map_or("", |e| e.url.trim());
        let url = if set.is_empty() { p.default_url() } else { set };
        url.to_owned()
    }

    pub fn preset(&self, id: PresetId) -> Option<&ModelPreset> {
        self.presets.iter().find(|p| p.id == id)
    }

    pub fn preset_mut(&mut self, id: PresetId) -> Option<&mut ModelPreset> {
        self.presets.iter_mut().find(|p| p.id == id)
    }

    /// The meaning's language as a name, for the prompt.
    pub fn explain_language(&self) -> Option<&'static str> {
        self.tasks
            .explain
            .language
            .as_deref()
            .and_then(language_name)
    }

    /// Saves a new model, named after what it runs; its id.
    pub fn add_preset(&mut self, provider: Provider, model: &str) -> PresetId {
        let next = self.presets.iter().map(|p| p.id.0).max().unwrap_or(0);
        let id = PresetId(next.saturating_add(1));
        self.presets.push(ModelPreset {
            id,
            name: ModelPreset::suggested_name(provider, model),
            provider,
            model: model.trim().to_owned(),
            ..ModelPreset::default()
        });
        id
    }

    /// Forgets a model; tasks that used it are left without one.
    pub fn remove_preset(&mut self, id: PresetId) {
        self.presets.retain(|p| p.id != id);
        self.unassign_missing();
    }

    fn unassign_missing(&mut self) {
        prune(&mut self.tasks.models, &self.presets);
        self.tasks.backups.prune(&self.presets);
        self.tasks.skills.prune(&self.presets);
    }

    /// Runs on every edit, so text being typed is never rewritten.
    pub fn sanitize(&mut self) {
        for p in Provider::SERVERS {
            if let Some(e) = self.endpoint_mut(p) {
                cut(&mut e.url, TEXT_MAX);
            }
        }
        let mut seen = std::collections::HashSet::new();
        self.presets.retain(|p| p.id.0 > 0 && seen.insert(p.id));
        self.presets.truncate(Self::PRESETS_MAX);
        for p in &mut self.presets {
            p.sanitize();
        }
        self.unassign_missing();
        self.tasks.skills.sanitize(&self.presets);
        drop_unknown(&mut self.tasks.explain.language);
        drop_unknown(&mut self.tasks.study.language);
        let batch = &mut self.tasks.organize.batch;
        *batch = (*batch).clamp(OrganizeTask::BATCH_MIN, OrganizeTask::BATCH_MAX);
        let translate = &mut self.tasks.translate;
        drop_unknown(&mut translate.language);
        let mut seen = std::collections::HashSet::new();
        translate
            .recent
            .retain(|t| language_name(t).is_some() && seen.insert(t.clone()));
        translate.recent.truncate(TranslateTask::RECENT_MAX);
        let pages = &mut self.tasks.pages;
        pages.dwell = pages.dwell.clamp(PageRun::DWELL_MIN, PageRun::DWELL_MAX);
        pages.at_once = pages.at_once.clamp(1, PageRun::AT_ONCE_MAX);
        pages.per_request = pages.per_request.clamp(1, PageRun::PER_REQUEST_MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_addresses_fall_back_to_each_provider_s_default() {
        let mut ai = AiSettings::default();
        assert_eq!(ai.url(Provider::Gemini), Provider::Gemini.default_url());
        assert_eq!(ai.url(Provider::Custom), "");
        ai.ollama.url = "  http://box:11434/v1  ".into();
        assert_eq!(ai.url(Provider::Ollama), "http://box:11434/v1");
        assert_eq!(ai.endpoint(Provider::Apple), None);
    }

    #[test]
    fn a_mac_starts_with_apple_explaining_and_others_with_nothing() {
        let ai = AiSettings::default();
        match cfg!(target_os = "macos") {
            true => assert_eq!(
                ai.model(TaskId::Explain).map(|p| p.provider),
                Some(Provider::Apple)
            ),
            false => assert!(ai.presets.is_empty() && ai.tasks.models.is_empty()),
        }
    }

    #[test]
    fn presets_get_fresh_ids_and_removal_unassigns_tasks() {
        let mut ai = AiSettings {
            presets: Vec::new(),
            ..AiSettings::default()
        };
        let fast = ai.add_preset(Provider::OpenAi, " gpt-4.1-nano ");
        let local = ai.add_preset(Provider::Ollama, "");
        assert_ne!(fast, local);
        assert_eq!(
            ai.preset(fast).map(|p| p.name.as_str()),
            Some("OpenAI · gpt-4.1-nano")
        );
        assert_eq!(ai.preset(local).map(ModelPreset::model), Some(""));
        ai.tasks.choose(TaskId::Explain, Some(fast));
        // Organize follows Explain until it has its own.
        assert_eq!(ai.model(TaskId::Organize).map(|p| p.id), Some(fast));
        ai.tasks.choose(TaskId::Organize, Some(local));
        assert_eq!(ai.model(TaskId::Organize).map(|p| p.id), Some(local));
        ai.remove_preset(fast);
        assert_eq!(ai.tasks.choice(TaskId::Explain), None);
        assert_eq!(ai.add_preset(Provider::Gemini, ""), PresetId(3));
    }

    #[test]
    fn sanitize_clamps_and_drops_what_no_longer_points_anywhere() {
        let mut ai = AiSettings::default();
        let id = ai.add_preset(Provider::Custom, "m");
        if let Some(p) = ai.preset_mut(id) {
            p.at_once = 99;
            p.cards_per_request = 0;
            p.name = "n".repeat(ModelPreset::NAME_MAX + 5);
        }
        let twin = ai.presets.first().cloned().unwrap_or_default();
        ai.presets.push(twin);
        let organize = vec![PresetId(77), id, id];
        ai.tasks.models.insert(TaskId::Organize, organize);
        ai.tasks.organize.batch = 1;
        ai.tasks.explain.language = Some("xx".into());
        ai.custom.url = "h".into();
        ai.sanitize();
        let p = ai.preset(id).cloned().unwrap_or_default();
        assert_eq!(p.at_once, ModelPreset::AT_ONCE_MAX);
        assert_eq!(p.cards_per_request, 1);
        let old: Option<ModelPreset> = serde_json::from_str(r#"{"id": 4}"#).ok();
        assert_eq!(old.map(|p| p.cards_per_request), Some(10), "older files");
        assert_eq!(p.name.chars().count(), ModelPreset::NAME_MAX);
        let ids: std::collections::HashSet<_> = ai.presets.iter().map(|p| p.id).collect();
        assert_eq!(ids.len(), ai.presets.len(), "ids are unique");
        assert_eq!(ai.tasks.models.get(&TaskId::Organize), Some(&vec![id]));
        assert_eq!(ai.tasks.organize.batch, OrganizeTask::BATCH_MIN);
        assert_eq!(ai.tasks.explain.language, None);
        assert_eq!(ai.custom.url, "h", "text being typed survives");
    }

    #[test]
    fn a_language_tag_is_named() {
        assert_eq!(language_name("my"), Some("Burmese"));
        assert_eq!(language_name("zh-Hant"), Some("Chinese (Traditional)"));
        assert_eq!(language_name("??"), None);
    }

    #[test]
    fn providers_keep_their_names_in_the_file() {
        let names: Vec<String> = Provider::ALL
            .iter()
            .filter_map(|p| serde_json::to_string(p).ok())
            .collect();
        let want = [
            "apple",
            "openai",
            "gemini",
            "openrouter",
            "ollama",
            "lm_studio",
            "custom",
        ];
        let want: Vec<String> = want.iter().map(|w| format!("\"{w}\"")).collect();
        assert_eq!(names, want);
    }

    #[test]
    fn page_tasks_follow_explain_until_given_their_own() {
        let mut ai = AiSettings::default();
        let explain = ai.add_preset(Provider::Ollama, "qwen3");
        let own = ai.add_preset(Provider::OpenAi, "");
        ai.tasks.choose(TaskId::Explain, Some(explain));
        let skill = crate::Skill::Builtin(PageTask::Translate);
        ai.tasks.skills.set_own(skill, Some(own));
        let id = |ai: &AiSettings, t: PageTask| ai.page_model(t).map(|p| p.id);
        assert_eq!(id(&ai, PageTask::Summarize), Some(explain));
        assert_eq!(id(&ai, PageTask::Translate), Some(own));
        ai.remove_preset(own);
        assert_eq!(ai.tasks.skills.own(skill), None);
        assert_eq!(id(&ai, PageTask::Translate), Some(explain));
    }

    #[test]
    fn translate_keeps_recent_languages_and_page_runs_stay_in_range() {
        let mut ai = AiSettings::default();
        let t = &mut ai.tasks.translate;
        for tag in ["my", "fr", "de", "my", "ja", "es", "it", "xx"] {
            t.pick(tag);
        }
        assert_eq!(t.language.as_deref(), Some("it"));
        assert_eq!(t.recent, ["it", "es", "ja", "my", "de"]);
        t.recent.push("zz".into());
        t.language = Some("zz".into());
        ai.tasks.pages = PageRun {
            dwell: 1,
            at_once: 99,
            per_request: 0,
        };
        ai.sanitize();
        assert_eq!(ai.tasks.translate.language, None);
        assert!(!ai.tasks.translate.recent.contains(&"zz".to_owned()));
        assert_eq!(ai.tasks.pages.dwell, PageRun::DWELL_MIN);
        assert_eq!(ai.tasks.pages.at_once, PageRun::AT_ONCE_MAX);
        assert_eq!(PageRun::default().at_once, 5);
        assert_eq!(ai.tasks.pages.per_request, 1);
        assert_eq!(PageRun::default().per_request, 5);
        assert_eq!(ai.tasks.simplify.start, PageStart::Ask);
    }
}
