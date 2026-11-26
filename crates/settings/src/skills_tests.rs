use super::*;
use crate::{BarItem, Provider};

fn bare() -> AiSettings {
    AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    }
}

#[test]
fn a_skill_asks_its_own_model_then_selection_ai_s_route() {
    let mut ai = bare();
    let ids = |ai: &AiSettings, s| ai.skill_route(s).iter().map(|p| p.id).collect::<Vec<_>>();
    let translate = Skill::Builtin(PageTask::Translate);
    assert!(ids(&ai, translate).is_empty(), "no model at all: none");
    let a = ai.add_preset(Provider::Ollama, "a");
    let b = ai.add_preset(Provider::Ollama, "b");
    let c = ai.add_preset(Provider::Ollama, "c");
    ai.tasks.choose(TaskId::SelectionAi, Some(a));
    assert_eq!(
        ids(&ai, translate),
        [a, b, c],
        "Selection AI's, then backups"
    );
    ai.tasks.skills.set_own(translate, Some(c));
    assert_eq!(ids(&ai, translate), [c, a, b]);
    let summarize = Skill::Builtin(PageTask::Summarize);
    assert_eq!(ids(&ai, summarize), [a, b, c], "others keep the shared one");
    ai.remove_preset(c);
    assert_eq!(ids(&ai, translate), [a, b], "a lost model falls back");
}

#[test]
fn an_own_model_with_no_shared_one_still_has_backups() {
    let mut ai = bare();
    let a = ai.add_preset(Provider::Ollama, "a");
    let b = ai.add_preset(Provider::Ollama, "b");
    let id = ai.tasks.skills.add().unwrap_or_default();
    let skill = Skill::Custom(id);
    ai.tasks.skills.set_own(skill, Some(b));
    let ids: Vec<_> = ai.skill_route(skill).iter().map(|p| p.id).collect();
    assert_eq!(ids, [b, a]);
}

#[test]
fn custom_skills_get_fresh_ids_and_keys_round_trip() {
    let mut skills = Skills::default();
    let one = skills.add().unwrap_or_default();
    let two = skills.add().unwrap_or_default();
    assert_eq!((one, two), (SkillId(1), SkillId(2)));
    skills.remove(one);
    assert_eq!(skills.add(), Some(SkillId(3)));
    for skill in [Skill::Builtin(PageTask::Simplify), Skill::Custom(two)] {
        assert_eq!(Skill::parse(&skill.key()), Some(skill));
    }
    assert_eq!(Skill::parse("7"), Some(Skill::Custom(SkillId(7))));
    assert_eq!(Skill::parse("0"), None);
    assert_eq!(Skill::parse("shout"), None);
    while skills.add().is_some() {}
    assert_eq!(skills.custom.len(), Skills::MAX);
}

#[test]
fn a_blank_name_reads_as_untitled_and_long_text_is_cut() {
    let mut ai = bare();
    let id = ai.tasks.skills.add().unwrap_or_default();
    assert_eq!(
        ai.skill_name(Skill::Custom(id)).as_deref(),
        Some("Untitled skill")
    );
    if let Some(s) = ai.tasks.skills.get_mut(id) {
        s.name = "n".repeat(99);
        s.prompt = " Explain like I'm five. ".into();
    }
    ai.sanitize();
    let s = ai.tasks.skills.get(id).cloned().unwrap_or_default();
    assert_eq!(s.name.chars().count(), CustomSkill::NAME_MAX);
    assert_eq!(s.prompt, " Explain like I'm five. ", "typing survives");
    assert_eq!(ai.skill_name(Skill::Custom(SkillId(9))), None);
}

#[test]
fn v8_s_page_models_become_the_skills_own() {
    let v8 = r#"{"version":8,
        "ai":{"presets":[{"id":1,"provider":"ollama"},{"id":2,"provider":"openai"}],
            "tasks":{"models":{"explain":[1],"translate":[2,1],"summarize":[1]}}},
        "study":{"highlights":{"bar":["copy","explain","note"]}}}"#;
    let got = crate::load(v8);
    assert!(got.notes.is_empty(), "{:?}", got.notes);
    let ai = got.settings.ai;
    assert_eq!(ai.tasks.models.len(), 1, "{:?}", ai.tasks.models);
    assert_eq!(ai.tasks.skills.translate, Some(PresetId(2)));
    assert_eq!(ai.tasks.skills.summarize, Some(PresetId(1)));
    assert_eq!(ai.tasks.skills.simplify, None);
    let page = |t| ai.page_model(t).map(|p| p.id.0);
    assert_eq!(PageTask::ALL.map(page), [1, 2, 1].map(Some));
    let bar = got.settings.study.highlights.bar;
    let want = [
        BarItem::Copy,
        BarItem::Explain,
        BarItem::Skills,
        BarItem::Note,
    ];
    assert_eq!(bar, want);
}

#[test]
fn selection_answers_pin_by_default_in_new_and_old_settings() {
    assert!(Skills::default().pin_by_default);
    let old: Skills = serde_json::from_str(r#"{"custom":[]}"#).unwrap();
    assert!(old.pin_by_default);
    let changed: Skills = serde_json::from_str(r#"{"pin_by_default":false}"#).unwrap();
    assert!(!changed.pin_by_default);
}
