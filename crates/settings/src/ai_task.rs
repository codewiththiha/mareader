//! The AI tasks as data: one row each, and the models each one asks.
//!
//! Plain data, like `ai_job.rs`, so it sits below the crate that runs the
//! work and the one that commands it.

use std::collections::BTreeMap;
use std::time::Duration;

use serde::{Deserialize, Deserializer, Serialize};

use crate::ai::{AiSettings, AiTasks, ModelPreset, PageTask, PresetId};

/// Every AI task. Its row is [`TaskId::spec`]; its models, [`AiTasks::models`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskId {
    Explain,
    Organize,
    Summarize,
    Translate,
    Simplify,
    StudyCards,
    StudyJudge,
}

/// Who is waiting, which decides the order in the queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// The reader is looking at a spinner: Explain, a recall's verdict.
    Waited,
    /// Asked for and shown as it comes: a page rewrite.
    Asked,
    /// Made ahead or in bulk: decks ahead, the organizer.
    Ahead,
}

/// How often a task asks again, and how long it waits first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Retry {
    /// Times an unreadable answer is asked again.
    pub unreadable: u8,
    /// Pauses before asking again when a server is briefly away.
    pub waits: &'static [Duration],
}

impl Retry {
    /// Pauses a server is given to come back when no one watches a spinner.
    pub const WAITS: [Duration; 2] = [Duration::from_secs(3), Duration::from_secs(12)];

    /// The reader is waiting: one re-ask, one short pause.
    pub const WAITED: Retry = Retry {
        unreadable: 1,
        waits: &[Duration::from_secs(2)],
    };

    /// Work made ahead, or shown as it comes: one re-ask, then [`Self::WAITS`].
    pub const AHEAD: Retry = Retry {
        unreadable: 1,
        waits: &Self::WAITS,
    };
}

/// One task's row: its words, its place in the queue, its retry, its fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TaskSpec {
    /// Its name in Settings, as "AI Dictionary".
    pub label: &'static str,
    /// What it is busy with, as "organizing your library".
    pub doing: &'static str,
    pub class: Class,
    pub retry: Retry,
    /// Whose models it takes when it has none of its own.
    pub follows: Option<TaskId>,
}

const fn row(label: &'static str, doing: &'static str, class: Class) -> TaskSpec {
    let retry = match class {
        Class::Waited => Retry::WAITED,
        Class::Asked | Class::Ahead => Retry::AHEAD,
    };
    TaskSpec {
        label,
        doing,
        class,
        retry,
        follows: Some(TaskId::Explain),
    }
}

const EXPLAIN: TaskSpec = TaskSpec {
    follows: None,
    ..row("AI Dictionary", "explaining a word", Class::Waited)
};
const ORGANIZE: TaskSpec = row("Organize library", "organizing your library", Class::Ahead);
const SUMMARIZE: TaskSpec = row("Summarize", "summarizing a page", Class::Asked);
const TRANSLATE: TaskSpec = row("Translate", "translating a page", Class::Asked);
const SIMPLIFY: TaskSpec = row("Simplify", "simplifying a page", Class::Asked);
const STUDY_CARDS: TaskSpec = row("Study cards", "writing study cards", Class::Ahead);
const STUDY_JUDGE: TaskSpec = row("Study judge", "judging a recall", Class::Waited);

impl TaskId {
    pub const ALL: [TaskId; 7] = [
        Self::Explain,
        Self::Organize,
        Self::Summarize,
        Self::Translate,
        Self::Simplify,
        Self::StudyCards,
        Self::StudyJudge,
    ];

    /// Exhaustive: a task without a row doesn't compile.
    pub const fn spec(self) -> &'static TaskSpec {
        match self {
            Self::Explain => &EXPLAIN,
            Self::Organize => &ORGANIZE,
            Self::Summarize => &SUMMARIZE,
            Self::Translate => &TRANSLATE,
            Self::Simplify => &SIMPLIFY,
            Self::StudyCards => &STUDY_CARDS,
            Self::StudyJudge => &STUDY_JUDGE,
        }
    }
}

impl From<PageTask> for TaskId {
    fn from(task: PageTask) -> Self {
        match task {
            PageTask::Summarize => Self::Summarize,
            PageTask::Translate => Self::Translate,
            PageTask::Simplify => Self::Simplify,
        }
    }
}

/// The reader's words when `task` has no model to ask.
pub fn no_model(task: TaskId) -> String {
    let doing = task.spec().doing;
    format!("No AI model is set for {doing}. Choose one in Settings › AI.")
}

/// Each task's models, first choice first.
pub type Routes = BTreeMap<TaskId, Vec<PresetId>>;

/// Routes from the file; a task this build doesn't know is skipped, not fatal.
pub(crate) fn known<'de, D: Deserializer<'de>>(d: D) -> Result<Routes, D::Error> {
    let raw = BTreeMap::<String, Vec<PresetId>>::deserialize(d)?;
    let task = |k: String| serde_json::from_value::<TaskId>(serde_json::Value::String(k)).ok();
    Ok(raw
        .into_iter()
        .filter_map(|(k, ids)| task(k).map(|t| (t, ids)))
        .collect())
}

/// Forgets ids no saved model has, repeats, and tasks left with none.
pub(crate) fn prune(routes: &mut Routes, presets: &[ModelPreset]) {
    for ids in routes.values_mut() {
        let mut seen = Vec::new();
        ids.retain(|id| {
            let keep = presets.iter().any(|p| p.id == *id) && !seen.contains(id);
            seen.push(*id);
            keep
        });
    }
    routes.retain(|_, ids| !ids.is_empty());
}

impl AiTasks {
    /// The model chosen for `task` itself, not one it follows.
    pub fn choice(&self, task: TaskId) -> Option<PresetId> {
        self.models.get(&task).and_then(|ids| ids.first().copied())
    }

    /// Makes `id` the first choice, keeping the fallbacks; none clears all.
    pub fn choose(&mut self, task: TaskId, id: Option<PresetId>) {
        let Some(id) = id else {
            self.models.remove(&task);
            return;
        };
        let ids = self.models.entry(task).or_default();
        ids.retain(|i| *i != id);
        ids.insert(0, id);
    }
}

impl AiSettings {
    /// The models `task` may ask, in order: its own, then those it follows.
    pub fn route(&self, task: TaskId) -> Vec<&ModelPreset> {
        let own = self.tasks.models.get(&task).into_iter().flatten();
        let mut out: Vec<&ModelPreset> = own.filter_map(|id| self.preset(*id)).collect();
        if let Some(parent) = task.spec().follows {
            for p in self.route(parent) {
                if !out.iter().any(|q| q.id == p.id) {
                    out.push(p);
                }
            }
        }
        out
    }

    /// The first model `task` asks, if any.
    pub fn model(&self, task: TaskId) -> Option<&ModelPreset> {
        self.route(task).into_iter().next()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Provider;

    fn bare() -> AiSettings {
        AiSettings {
            presets: Vec::new(),
            tasks: AiTasks::default(),
            ..AiSettings::default()
        }
    }

    #[test]
    fn every_task_but_explain_follows_explain() {
        for task in TaskId::ALL {
            let want = (task != TaskId::Explain).then_some(TaskId::Explain);
            assert_eq!(task.spec().follows, want, "{task:?}");
        }
        let labels: std::collections::HashSet<_> =
            TaskId::ALL.iter().map(|t| t.spec().label).collect();
        assert_eq!(labels.len(), TaskId::ALL.len(), "labels are unique");
    }

    #[test]
    fn a_missing_model_is_named_by_what_it_would_do() {
        let none = no_model(TaskId::StudyJudge);
        assert_eq!(
            none,
            "No AI model is set for judging a recall. Choose one in Settings › AI."
        );
    }

    #[test]
    fn the_spinner_tasks_wait_once_and_the_rest_twice() {
        let waits = |t: TaskId| t.spec().retry.waits.len();
        assert_eq!(TaskId::Explain.spec().class, Class::Waited);
        assert_eq!(TaskId::StudyJudge.spec().class, Class::Waited);
        assert_eq!(TaskId::Translate.spec().class, Class::Asked);
        assert_eq!(TaskId::StudyCards.spec().class, Class::Ahead);
        assert_eq!((waits(TaskId::Explain), waits(TaskId::Organize)), (1, 2));
    }

    #[test]
    fn a_route_is_its_own_models_then_explain_s_once_each() {
        let mut ai = bare();
        let a = ai.add_preset(Provider::Ollama, "a");
        let b = ai.add_preset(Provider::Ollama, "b");
        let c = ai.add_preset(Provider::Ollama, "c");
        assert!(ai.route(TaskId::Organize).is_empty());
        ai.tasks.models.insert(TaskId::Explain, vec![a, b]);
        ai.tasks.models.insert(TaskId::Organize, vec![c, a]);
        let ids = |t| ai.route(t).iter().map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(ids(TaskId::Organize), [c, a, b]);
        assert_eq!(ids(TaskId::StudyJudge), [a, b]);
        assert_eq!(ai.model(TaskId::Organize).map(|p| p.id), Some(c));
    }

    #[test]
    fn choosing_keeps_the_fallbacks_and_none_clears_them() {
        let mut tasks = AiTasks::default();
        let (a, b) = (PresetId(1), PresetId(2));
        tasks.models.insert(TaskId::Translate, vec![a, b]);
        tasks.choose(TaskId::Translate, Some(b));
        assert_eq!(tasks.models.get(&TaskId::Translate), Some(&vec![b, a]));
        assert_eq!(tasks.choice(TaskId::Translate), Some(b));
        tasks.choose(TaskId::Translate, None);
        assert_eq!(tasks.choice(TaskId::Translate), None);
        assert!(tasks.models.is_empty());
    }

    #[test]
    fn v7_s_tasks_keep_the_models_they_had() {
        let v7 = r#"{"version":7,"ai":{
            "presets":[{"id":1,"provider":"ollama"},{"id":2,"provider":"openai"},
                {"id":3,"provider":"gemini"}],
            "tasks":{"explain":{"preset":1,"language":"my"},"organize":{"preset":2,"batch":20},
                "summarize":{"preset":null},"translate":{"preset":9,"language":"fr"},
                "simplify":{"preset":3},"study":{"preset":2,"language":"ja"}}}}"#;
        let got = crate::load(v7);
        assert!(got.notes.is_empty(), "{:?}", got.notes);
        let (ai, version) = (got.settings.ai, got.settings.version);
        let model = |t| ai.model(t).map(|p| p.id.0);
        let all = TaskId::ALL.map(model);
        // Summarize had none and translate a lost one: both follow Explain.
        let want: [Option<u32>; 7] = [1, 2, 1, 1, 3, 2, 2].map(Some);
        assert_eq!(all, want);
        assert_eq!(ai.tasks.models.len(), 5, "{:?}", ai.tasks.models);
        assert_eq!(ai.tasks.organize.batch, 20);
        let languages = [&ai.tasks.explain.language, &ai.tasks.study.language];
        assert_eq!(languages.map(Option::as_deref), [Some("my"), Some("ja")]);
        assert_eq!(ai.tasks.translate.language.as_deref(), Some("fr"));
        assert_eq!(version, crate::SCHEMA_VERSION);
    }

    #[test]
    fn a_task_a_newer_build_added_is_skipped_not_fatal() {
        let file = r#"{"models":{"explain":[1],"read_aloud":[2]},"organize":{"batch":20}}"#;
        let tasks: AiTasks = serde_json::from_str(file).unwrap_or_default();
        assert_eq!(tasks.choice(TaskId::Explain), Some(PresetId(1)));
        assert_eq!(tasks.models.len(), 1);
        assert_eq!(tasks.organize.batch, 20);
    }
}
