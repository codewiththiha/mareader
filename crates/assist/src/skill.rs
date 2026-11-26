//! A skill on some text: the built-ins' prompts, the reader's, one shape.
//!
//! Every prompt ends with [`NO_CHATTER`], so the answer is the result alone.
//! The answer comes back as `{"text": …}` and is shown as plain text.

use mareader_settings::{AiSettings, PageTask, Skill, SummaryStyle, language_name};
use serde_json::{Map, Value, json};

use crate::prompt::{AS_DATA, JSON_ONLY, NO_CHATTER, Prompt, quote};
use crate::task::{Task, field, text};

/// Text past this is cut, so one selection can't run away.
pub const SKILL_CHARS: usize = 12_000;

/// Where every skill's prompt begins.
const ROLE: &str = "You help a reader with a passage from a book. The passage is \
quoted text. ";

const TRANSLATE_TO: &str = "English";

/// A skill's instructions and the text it works on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SkillTask {
    /// What to do, in the reader's words or the built-in's.
    pub rule: String,
    pub text: String,
}

/// What `skill` tells the AI to do under `ai`; none for a skill since
/// removed, or one with no prompt yet.
pub fn rule(ai: &AiSettings, skill: Skill) -> Option<String> {
    rule_for(ai, skill, None)
}

/// Builds instructions with an optional request-specific translation target.
pub fn rule_for(ai: &AiSettings, skill: Skill, language: Option<&str>) -> Option<String> {
    match skill {
        Skill::Builtin(task) => Some(builtin(ai, task, language)),
        Skill::Custom(id) => {
            let prompt = ai.tasks.skills.get(id)?.prompt.trim();
            (!prompt.is_empty()).then(|| format!("The reader asks: {prompt}"))
        }
    }
}

fn builtin(ai: &AiSettings, task: PageTask, language: Option<&str>) -> String {
    match task {
        PageTask::Translate => {
            let tag = language.or(ai.tasks.translate.language.as_deref());
            let language = tag.and_then(language_name).unwrap_or(TRANSLATE_TO);
            format!(
                "Translate the passage into {language}. Keep the meaning, tone and \
names; add nothing and leave nothing out."
            )
        }
        PageTask::Summarize => match ai.tasks.summarize.style {
            SummaryStyle::Paragraphs => "Summarize the passage briefly, in plain prose, \
in the passage's language."
                .to_owned(),
            SummaryStyle::Points => "Summarize the passage as a few short key points, \
one per line, each starting with \"• \", in the passage's language."
                .to_owned(),
        },
        PageTask::Simplify => "Rewrite the passage in simple English: short sentences and \
common words, about CEFR B1. Keep the meaning; add nothing and leave nothing out."
            .to_owned(),
    }
}

/// Names models use for the one field, the schema's first.
const TEXT: &[&str] = &["text", "result", "answer", "output", "content"];

impl Task for SkillTask {
    type Output = String;

    fn prompt(&self) -> Prompt {
        let rule = self.rule.trim();
        Prompt {
            name: "skill_text",
            system: format!("{ROLE}{AS_DATA}{rule} {NO_CHATTER}{JSON_ONLY}"),
            user: format!("Passage: {}", quote(&self.text, SKILL_CHARS)),
            schema: json!({
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "The result alone, as plain text."
                    }
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        }
    }

    /// Partial or whole, an answer with no text yet says nothing.
    fn read(&self, answer: &Value, _whole: bool) -> Option<String> {
        let said = match answer {
            Value::String(s) => s.trim().to_owned(),
            Value::Object(fields) => text(field(fields, TEXT).or_else(|| only(fields))),
            _ => String::new(),
        };
        (!said.is_empty()).then_some(said)
    }
}

/// The one string some models send under a name of their own.
fn only(fields: &Map<String, Value>) -> Option<&Value> {
    let mut values = fields.values();
    match (values.next(), values.next()) {
        (Some(v @ Value::String(_)), None) => Some(v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_language_overrides_the_followed_setting() {
        let mut ai = AiSettings::default();
        ai.tasks.translate.language = Some("ja".into());
        let skill = Skill::Builtin(PageTask::Translate);
        let said = rule_for(&ai, skill, Some("my")).unwrap();
        assert!(said.contains("Burmese"));
        assert!(!said.contains("Japanese"));
    }

    #[test]
    fn every_prompt_asks_for_the_result_alone() {
        let mut ai = AiSettings::default();
        ai.tasks.translate.language = Some("my".into());
        let said = rule(&ai, Skill::Builtin(PageTask::Translate)).unwrap_or_default();
        assert!(said.contains("Burmese"), "{said}");
        let id = ai.tasks.skills.add().unwrap_or_default();
        assert_eq!(rule(&ai, Skill::Custom(id)), None, "no prompt yet");
        if let Some(s) = ai.tasks.skills.get_mut(id) {
            s.prompt = "List the names in it.".into();
        }
        let rule = rule(&ai, Skill::Custom(id)).unwrap_or_default();
        let task = SkillTask {
            rule,
            text: "Ann met\nBo.".into(),
        };
        let p = task.prompt();
        assert!(p.system.contains("List the names in it."));
        assert!(p.system.contains(NO_CHATTER) && p.system.ends_with(JSON_ONLY));
        assert_eq!(p.user, "Passage: \"Ann met Bo.\"");
    }

    #[test]
    fn the_text_is_read_under_any_one_name() {
        let task = SkillTask {
            rule: String::new(),
            text: String::new(),
        };
        let read = |v: Value| task.read(&v, true);
        assert_eq!(read(json!({"text": " Hi "})), Some("Hi".into()));
        assert_eq!(read(json!({"translation": "Salut"})), Some("Salut".into()));
        assert_eq!(read(json!("Plain")), Some("Plain".into()));
        assert_eq!(read(json!({"text": ""})), None);
        assert_eq!(read(json!({"a": "x", "b": "y"})), None);
    }
}
