//! Skills: what the AI does with a selection, or with a page.
//!
//! Three are built in, Translate, Summarize and Simplify, their prompts fixed
//! in code. The reader writes the rest: a name and a prompt each. Every skill
//! asks Selection AI's models; one given a model of its own asks that first,
//! then falls back along Selection AI's route and its backups.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::ai::{AiSettings, ModelPreset, PageTask, PresetId, cut};
use crate::ai_task::TaskId;

/// A custom skill's handle; menus and commands name it by this.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct SkillId(pub u32);

/// One skill, built in or the reader's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Skill {
    Builtin(PageTask),
    Custom(SkillId),
}

impl Skill {
    /// The built-ins, in the selection menu's order.
    pub const BUILTIN: [Skill; 3] = [
        Self::Builtin(PageTask::Translate),
        Self::Builtin(PageTask::Summarize),
        Self::Builtin(PageTask::Simplify),
    ];

    /// Its name in commands: "translate", or "custom-3".
    pub fn key(self) -> String {
        match self {
            Self::Builtin(task) => task.key().to_owned(),
            Self::Custom(id) => format!("custom-{}", id.0),
        }
    }

    /// The skill [`Self::key`] names; a bare number names a custom one.
    pub fn parse(key: &str) -> Option<Skill> {
        let builtin = PageTask::ALL.into_iter().find(|t| t.key() == key);
        if let Some(task) = builtin {
            return Some(Self::Builtin(task));
        }
        let n = key.strip_prefix("custom-").unwrap_or(key);
        n.parse()
            .ok()
            .filter(|n| *n > 0)
            .map(|n| Self::Custom(SkillId(n)))
    }
}

/// A skill the reader wrote.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomSkill {
    pub id: SkillId,
    pub name: String,
    /// What the AI is told to do with the text.
    pub prompt: String,
    /// Its own model; none asks Selection AI's.
    pub model: Option<PresetId>,
}

impl CustomSkill {
    pub const NAME_MAX: usize = 40;
    pub const PROMPT_MAX: usize = 4_000;

    /// Its name, or a stand-in while it has none.
    pub fn title(&self) -> &str {
        match self.name.trim() {
            "" => "Untitled skill",
            name => name,
        }
    }
}

/// The skills' own models and the reader's skills.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Skills {
    /// Built-ins' own models; none asks Selection AI's.
    pub translate: Option<PresetId>,
    pub summarize: Option<PresetId>,
    pub simplify: Option<PresetId>,
    pub custom: Vec<CustomSkill>,
}

impl Skills {
    pub const MAX: usize = 24;

    /// The reader's skill `id`.
    pub fn get(&self, id: SkillId) -> Option<&CustomSkill> {
        self.custom.iter().find(|s| s.id == id)
    }

    pub fn get_mut(&mut self, id: SkillId) -> Option<&mut CustomSkill> {
        self.custom.iter_mut().find(|s| s.id == id)
    }

    /// The model `skill` asks first, if it has one of its own.
    pub fn own(&self, skill: Skill) -> Option<PresetId> {
        match skill {
            Skill::Builtin(PageTask::Translate) => self.translate,
            Skill::Builtin(PageTask::Summarize) => self.summarize,
            Skill::Builtin(PageTask::Simplify) => self.simplify,
            Skill::Custom(id) => self.get(id).and_then(|s| s.model),
        }
    }

    /// Gives `skill` a model of its own, or none.
    pub fn set_own(&mut self, skill: Skill, model: Option<PresetId>) {
        match skill {
            Skill::Builtin(PageTask::Translate) => self.translate = model,
            Skill::Builtin(PageTask::Summarize) => self.summarize = model,
            Skill::Builtin(PageTask::Simplify) => self.simplify = model,
            Skill::Custom(id) => {
                if let Some(s) = self.get_mut(id) {
                    s.model = model;
                }
            }
        }
    }

    /// A new, blank skill at the end; none once there are [`Self::MAX`].
    pub fn add(&mut self) -> Option<SkillId> {
        if self.custom.len() >= Self::MAX {
            return None;
        }
        let top = self.custom.iter().map(|s| s.id.0).max().unwrap_or(0);
        let id = SkillId(top.saturating_add(1));
        self.custom.push(CustomSkill {
            id,
            ..CustomSkill::default()
        });
        Some(id)
    }

    pub fn remove(&mut self, id: SkillId) {
        self.custom.retain(|s| s.id != id);
    }

    /// Forgets models that are not saved.
    pub(crate) fn prune(&mut self, presets: &[ModelPreset]) {
        let saved = |id: &PresetId| presets.iter().any(|p| p.id == *id);
        for own in [&mut self.translate, &mut self.summarize, &mut self.simplify] {
            *own = own.filter(saved);
        }
        for s in &mut self.custom {
            s.model = s.model.filter(saved);
        }
    }

    /// Unique ids, bounded text, at most [`Self::MAX`]; text being typed
    /// stays as it is.
    pub(crate) fn sanitize(&mut self, presets: &[ModelPreset]) {
        let mut seen = std::collections::HashSet::new();
        self.custom.retain(|s| s.id.0 > 0 && seen.insert(s.id));
        self.custom.truncate(Self::MAX);
        for s in &mut self.custom {
            cut(&mut s.name, CustomSkill::NAME_MAX);
            cut(&mut s.prompt, CustomSkill::PROMPT_MAX);
        }
        self.prune(presets);
    }
}

impl AiSettings {
    /// The models `skill` may ask, in order: its own, then Selection AI's
    /// route; each once.
    pub fn skill_route(&self, skill: Skill) -> Vec<&ModelPreset> {
        let own = self.tasks.skills.own(skill).and_then(|id| self.preset(id));
        let shared = self.route(TaskId::SelectionAi);
        // With no shared model, its own still falls back on the backups.
        let rest = match shared.is_empty() {
            true if own.is_some() => self.backups(),
            _ => shared,
        };
        let mut out: Vec<&ModelPreset> = own.into_iter().collect();
        for p in rest {
            if !out.iter().any(|q| q.id == p.id) {
                out.push(p);
            }
        }
        out
    }

    /// The first model `skill` asks, if any.
    pub fn skill_model(&self, skill: Skill) -> Option<&ModelPreset> {
        self.skill_route(skill).into_iter().next()
    }

    /// The model a page task asks first.
    pub fn page_model(&self, task: PageTask) -> Option<&ModelPreset> {
        self.skill_model(Skill::Builtin(task))
    }

    /// What menus and cards call `skill`; none for a skill since removed.
    pub fn skill_name(&self, skill: Skill) -> Option<String> {
        match skill {
            Skill::Builtin(task) => Some(task.label().to_owned()),
            Skill::Custom(id) => self.tasks.skills.get(id).map(|s| s.title().to_owned()),
        }
    }
}

/// v9 asks one Selection AI model for every skill; a model picked for a
/// page task stays that skill's own. The selection menu gains Ask AI.
pub(crate) fn v8_to_v9(root: &mut Map<String, Value>) {
    let tasks = (root.get_mut("ai"))
        .and_then(|ai| ai.get_mut("tasks"))
        .and_then(Value::as_object_mut);
    if let Some(tasks) = tasks {
        let mut own = Map::new();
        if let Some(models) = tasks.get_mut("models").and_then(Value::as_object_mut) {
            for task in ["summarize", "translate", "simplify"] {
                let first = models.remove(task).and_then(|ids| ids.get(0).cloned());
                if let Some(id) = first.filter(Value::is_u64) {
                    own.insert(task.to_owned(), id);
                }
            }
        }
        if let Value::Object(skills) = tasks.entry("skills").or_insert_with(|| json!({})) {
            for (task, id) in own {
                skills.entry(task).or_insert(id);
            }
        }
    }
    let bar = (root.get_mut("study"))
        .and_then(|s| s.get_mut("highlights"))
        .and_then(|h| h.get_mut("bar"))
        .and_then(Value::as_array_mut);
    let Some(bar) = bar else {
        return;
    };
    let item = json!("skills");
    if bar.contains(&item) {
        return;
    }
    let near = |name: &str| bar.iter().position(|v| v == name);
    let at = near("explain")
        .or_else(|| near("copy"))
        .map_or(0, |i| i + 1);
    bar.insert(at, item);
}

#[cfg(test)]
#[path = "skills_tests.rs"]
mod tests;
