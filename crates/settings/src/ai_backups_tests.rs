use super::*;
use crate::{Provider, TaskId};

fn bare() -> AiSettings {
    AiSettings {
        presets: Vec::new(),
        ..AiSettings::default()
    }
}

fn saved(n: usize) -> (AiSettings, Vec<PresetId>) {
    let mut ai = bare();
    let ids = (0..n).map(|i| ai.add_preset(Provider::Ollama, &format!("m{i}")));
    let ids = ids.collect();
    (ai, ids)
}

fn ids(list: Vec<&ModelPreset>) -> Vec<PresetId> {
    list.iter().map(|p| p.id).collect()
}

#[test]
fn the_automatic_list_is_the_three_oldest_and_keeps_filling() {
    let (mut ai, made) = saved(2);
    assert_eq!(ids(ai.backups()), made);
    let third = ai.add_preset(Provider::Gemini, "");
    let fourth = ai.add_preset(Provider::Gemini, "");
    assert_eq!(ids(ai.backups()), [made[0], made[1], third]);
    ai.remove_preset(made[0]);
    assert_eq!(ids(ai.backups()), [made[1], third, fourth]);
    assert!(!ai.tasks.backups.arranged());
}

#[test]
fn an_arranged_list_keeps_its_order_and_takes_no_new_models() {
    let (mut ai, made) = saved(3);
    ai.tasks.backups.arrange(vec![made[2], made[0], made[2]]);
    assert_eq!(ids(ai.backups()), [made[2], made[0]], "each once");
    ai.add_preset(Provider::Gemini, "");
    assert_eq!(ids(ai.backups()), [made[2], made[0]]);
    ai.remove_preset(made[2]);
    assert_eq!(ids(ai.backups()), [made[0]]);
    ai.tasks.backups.reset();
    assert_eq!(ai.backups().len(), Backups::AUTO);
}

#[test]
fn a_route_skips_a_task_s_own_model_in_the_backups() {
    let (mut ai, made) = saved(3);
    let (a, b, c) = (made[0], made[1], made[2]);
    ai.tasks.backups.arrange(vec![b, c, a]);
    ai.tasks.models.insert(TaskId::Explain, vec![a]);
    ai.tasks.models.insert(TaskId::SelectionAi, vec![c]);
    let route = |ai: &AiSettings, t| ids(ai.route(t));
    assert_eq!(route(&ai, TaskId::Explain), [a, b, c]);
    assert_eq!(
        route(&ai, TaskId::SelectionAi),
        [c, a, b],
        "own, followed, then the rest"
    );
    ai.tasks.models.remove(&TaskId::Explain);
    assert_eq!(route(&ai, TaskId::SelectionAi), [c, b, a]);
    assert!(
        route(&ai, TaskId::Explain).is_empty(),
        "no model of its own: still off"
    );
}

#[test]
fn the_list_round_trips_and_an_old_file_is_automatic() {
    let (mut ai, made) = saved(2);
    ai.tasks.backups.arrange(vec![made[1]]);
    let json = serde_json::to_string(&ai.tasks).unwrap_or_default();
    let back: crate::AiTasks = serde_json::from_str(&json).unwrap_or_default();
    assert_eq!(back.backups, ai.tasks.backups);
    let old: crate::AiTasks = serde_json::from_str("{}").unwrap_or_default();
    assert!(!old.backups.arranged());
}
