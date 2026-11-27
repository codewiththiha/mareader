//! Translation languages shared by selection and page menus.

use egui::{Context, CornerRadius, Id, Rect, Response, Ui};
use mareader_dict::LANGUAGES;
use mareader_settings::{AiSettings, PageTask, Provider, Skill, language_name};

use crate::side_list::{self, Shown, Spec};
use crate::widgets;

pub const FIND: &str = "Find a language";

#[derive(Clone, Default)]
pub struct Choices {
    pub items: Vec<(String, String)>,
    pub current: Option<String>,
}

/// Builds choices from the first model's capabilities, when reported.
pub fn choices(
    ai: &AiSettings,
    apple_languages: Option<&[String]>,
    dictionary: Option<&str>,
) -> Choices {
    let model = ai.skill_model(Skill::Builtin(PageTask::Translate));
    let known = model
        .filter(|m| m.provider == Provider::Apple)
        .and(apple_languages);
    let primary = |tag: &str| tag.split(['-', '_']).next().unwrap_or("").to_lowercase();
    let allowed = |tag: &str| {
        known.is_none_or(|langs| langs.iter().any(|lang| primary(lang) == primary(tag)))
    };
    let mut items: Vec<(String, String)> = Vec::new();
    let recent = ai
        .tasks
        .translate
        .language
        .iter()
        .chain(ai.tasks.translate.recent.iter());
    for tag in recent {
        if allowed(tag)
            && !items.iter().any(|(t, _)| t == tag)
            && let Some(name) = language_name(tag)
        {
            items.push((tag.clone(), name.to_owned()));
        }
    }
    for lang in LANGUAGES {
        if allowed(lang.tag) && !items.iter().any(|(t, _)| t == lang.tag) {
            items.push((lang.tag.to_owned(), lang.name.to_owned()));
        }
    }
    Choices {
        items,
        current: mareader_settings::effective_language(
            ai.tasks.translate.language.as_deref(),
            dictionary,
            &|tag| known.map(|_| allowed(tag)),
        )
        .map(str::to_owned),
    }
}

fn offered_id() -> Id {
    Id::new("translation-languages")
}

pub fn offer(ctx: &Context, choices: Choices) {
    ctx.data_mut(|d| d.insert_temp(offered_id(), choices));
}

pub fn offered(ctx: &Context) -> Choices {
    ctx.data(|d| d.get_temp(offered_id())).unwrap_or_default()
}

/// Draws a hoverable row and a type-to-search list of languages.
pub fn show(
    ui: &mut Ui,
    choices: &Choices,
    row: impl FnOnce(&mut Ui) -> Response,
) -> Option<String> {
    show_beside(ui, choices, None, row)
}

/// Draws the language list outside an enclosing menu, when given.
pub fn show_beside(
    ui: &mut Ui,
    choices: &Choices,
    parent: Option<Rect>,
    row: impl FnOnce(&mut Ui) -> Response,
) -> Option<String> {
    show_beside_action(ui, choices, parent, row).picked
}

/// The trigger and the chosen submenu item; clicking the trigger is a
/// separate action from choosing a language.
pub fn show_beside_action(
    ui: &mut Ui,
    choices: &Choices,
    parent: Option<Rect>,
    row: impl FnOnce(&mut Ui) -> Response,
) -> Shown<String> {
    let spec = Spec {
        items: &choices.items,
        find: FIND,
        none: "No language matches",
        width: 196.0,
        corners: CornerRadius::same(8),
    };
    side_list::show_beside(ui, spec, parent, row, |ui, tag, name| {
        widgets::menu_check_row(ui, name, choices.current.as_ref() == Some(tag)).clicked()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_dictionary_until_a_manual_choice_and_falls_back_when_needed() {
        let mut ai = AiSettings::default();
        assert_eq!(
            choices(&ai, None, Some("fr")).current.as_deref(),
            Some("fr")
        );
        assert_eq!(choices(&ai, None, None).current.as_deref(), Some("en"));
        ai.tasks.translate.pick("my");
        assert_eq!(
            choices(&ai, None, Some("fr")).current.as_deref(),
            Some("my")
        );
        let id = ai.add_preset(Provider::Apple, "");
        ai.tasks
            .choose(mareader_settings::TaskId::SelectionAi, Some(id));
        let langs = vec!["en-US".into(), "ja-JP".into()];
        assert_eq!(
            choices(&ai, Some(&langs), Some("fr")).current.as_deref(),
            Some("en")
        );
        assert_eq!(ai.tasks.translate.language.as_deref(), Some("my"));
        assert_eq!(
            choices(&ai, None, Some("fr")).current.as_deref(),
            Some("my")
        );
    }

    #[test]
    fn unknown_model_offers_catalog_and_known_apple_is_filtered() {
        let mut ai = AiSettings::default();
        let unknown = choices(&ai, None, None);
        assert!(unknown.items.iter().any(|(tag, _)| tag == "my"));
        let id = ai.add_preset(Provider::Apple, "");
        ai.tasks
            .choose(mareader_settings::TaskId::SelectionAi, Some(id));
        let langs = vec!["en-US".into(), "ja-JP".into()];
        let filtered = choices(&ai, Some(&langs), None);
        assert!(filtered.items.iter().any(|(tag, _)| tag == "ja"));
        assert!(!filtered.items.iter().any(|(tag, _)| tag == "my"));
        let id = ai.add_preset(Provider::Ollama, "");
        ai.tasks
            .choose(mareader_settings::TaskId::SelectionAi, Some(id));
        let unknown = choices(&ai, Some(&langs), None);
        assert!(unknown.items.iter().any(|(tag, _)| tag == "my"));
    }
}
