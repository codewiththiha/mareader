//! The Skills tab: the built-ins' models, and the reader's own skills.
//!
//! A built-in's prompt is fixed; only the model it asks can change. A custom
//! skill is a name, a prompt and, if the reader wants, a model of its own.
//! Every skill asks Selection AI's model unless it has one.

use egui::{Align, Layout, RichText, Ui, vec2};
use mareader_settings::{AiSettings, CustomSkill, PageTask, Skill, SkillId, Skills, TaskId};

use super::ai::{model_row, no_models, preset_picker};
use super::{ROW_H, divider, group, note, row, section, switch_row};
use crate::icons::Icon;
use crate::input_field::{input, input_multiline};
use crate::theme::Tokens;
use crate::ui_command::Commands;
use crate::widgets;

/// What a skill with no model of its own asks.
const SHARED: &str = "Same as Selection AI";
const PROMPT_HINT: &str = "What should the AI do with the text? For example: explain the grammar.";

pub fn tab(ui: &mut Ui, ai: &mut AiSettings, out: &mut Commands) {
    if ai.presets.is_empty() {
        no_models(ui, out);
    }
    section(ui, "Default model");
    group(ui, |ui| model_row(ui, ai, TaskId::SelectionAi));
    section(ui, "Selection answers");
    group(ui, |ui| {
        switch_row(
            ui,
            "Pin new answers by default",
            "Saved with books in your library",
            &mut ai.tasks.skills.pin_by_default,
        );
    });
    builtins(ui, ai);
    custom(ui, ai);
}

/// Translate, Summarize, Simplify: a model each, or the shared one.
fn builtins(ui: &mut Ui, ai: &mut AiSettings) {
    section(ui, "Built-in skills");
    group(ui, |ui| {
        for (i, task) in [PageTask::Translate, PageTask::Summarize, PageTask::Simplify]
            .into_iter()
            .enumerate()
        {
            if i > 0 {
                divider(ui);
            }
            let skill = Skill::Builtin(task);
            let hint = "The model this skill asks first";
            row(ui, task.label(), hint, |ui| own_model(ui, ai, skill));
        }
    });
}

/// The model `skill` asks first: its own, or Selection AI's.
fn own_model(ui: &mut Ui, ai: &mut AiSettings, skill: Skill) {
    let was = ai.tasks.skills.own(skill);
    let mut chosen = was;
    let salt = format!("skill-{}-model", skill.key());
    preset_picker(ui, &salt, &ai.presets, &mut chosen, SHARED);
    if chosen != was {
        ai.tasks.skills.set_own(skill, chosen);
    }
}

/// The reader's skills, each a card, then a button for one more.
fn custom(ui: &mut Ui, ai: &mut AiSettings) {
    section(ui, "Your skills");
    let mut gone: Option<SkillId> = None;
    let ids: Vec<SkillId> = ai.tasks.skills.custom.iter().map(|s| s.id).collect();
    for (i, id) in ids.into_iter().enumerate() {
        if i > 0 {
            ui.add_space(10.0);
        }
        group(ui, |ui| {
            if card(ui, ai, id) {
                gone = Some(id);
            }
        });
    }
    if let Some(id) = gone {
        ai.tasks.skills.remove(id);
    }
    let count = ai.tasks.skills.custom.len();
    if count == 0 {
        note(
            ui,
            "Write a prompt once; it shows in the selection menu's Skills section.",
        );
    }
    ui.add_space(10.0);
    let t = Tokens::get(ui.ctx());
    let room = count < Skills::MAX;
    let button = widgets::primary_button(ui, Some(Icon::Plus), "Add a skill", t.accent, &t);
    if ui.add_enabled(room, button).clicked() {
        ai.tasks.skills.add();
    }
    if !room {
        note(ui, "That is as many as there can be.");
    }
}

/// One skill's name, prompt and model; true when it is to be removed.
fn card(ui: &mut Ui, ai: &mut AiSettings, id: SkillId) -> bool {
    let AiSettings { presets, tasks, .. } = ai;
    let Some(skill) = tasks.skills.get_mut(id) else {
        return false;
    };
    let gone = name_row(ui, skill);
    let t = Tokens::get(ui.ctx());
    ui.add_space(2.0);
    let label = RichText::new("Prompt").size(t.text_small).color(t.muted);
    ui.label(label);
    ui.add_space(4.0);
    input_multiline(&mut skill.prompt)
        .id(ui.id().with(("skill-prompt", id.0)))
        .hint(PROMPT_HINT)
        .rows(3)
        .char_limit(CustomSkill::PROMPT_MAX)
        .show(ui);
    ui.add_space(6.0);
    divider(ui);
    let hint = "The model this skill asks first";
    row(ui, "Model", hint, |ui| {
        let mut chosen = skill.model;
        let salt = format!("skill-custom-{}-model", id.0);
        preset_picker(ui, &salt, presets, &mut chosen, SHARED);
        skill.model = chosen;
    });
    gone
}

/// The name, editable in place, and the remove button.
fn name_row(ui: &mut Ui, skill: &mut CustomSkill) -> bool {
    let mut gone = false;
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), ROW_H + 4.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.set_min_height(ROW_H + 4.0);
            let label = format!("Remove {}", skill.title());
            gone = widgets::icon_button(ui, Icon::Remove, &label, false).clicked();
            ui.add_space(6.0);
            input(&mut skill.name)
                .id(ui.id().with(("skill-name", skill.id.0)))
                .hint("Name, as the menu shows it")
                .label("Skill name")
                .compact()
                .char_limit(CustomSkill::NAME_MAX)
                .width(ui.available_width())
                .show(ui);
        },
    );
    gone
}
