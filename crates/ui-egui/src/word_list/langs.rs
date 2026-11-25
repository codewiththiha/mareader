//! The word list's language columns: their order, and one more to add,
//! a pack's or the AI dictionary's.

use egui::{RichText, Ui};
use mareader_dict::{PACKS, Pack};
use mareader_engine::{command::Command, dict::Service as Dict, settings::Tab};
use mareader_settings::{WORD_LANGUAGES, WordList};

use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// Languages a list can add: every pack's, then those only the AI
/// dictionary keeps, less the list's own.
fn addable(ready: &[String], listed: &[String]) -> Vec<String> {
    let every: Vec<&'static Pack> = PACKS.iter().collect();
    let all = mareader_dict::targets(&every, ready).into_iter();
    let left = all.filter(|l| !listed.iter().any(|x| x == l));
    left.map(str::to_owned).collect()
}

/// The languages in column order, each movable, and one more to add.
pub(super) fn languages(
    ui: &mut Ui,
    list: &mut WordList,
    dict: &Dict,
    out: &mut Commands,
    close: &mut bool,
) {
    let t = Tokens::get(ui.ctx());
    let ready = dict.languages();
    let mut act = None;
    let n = list.languages.len();
    for (i, lang) in list.languages.iter().enumerate() {
        ui.horizontal(|ui| {
            let name = mareader_dict::language(lang);
            ui.label(format!("{}. {name}", i + 1));
            if !ready.contains(lang) {
                ui.label(
                    RichText::new("not downloaded")
                        .color(t.muted)
                        .size(t.text_small),
                );
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::small_icon_button(ui, Icon::Close, &format!("Remove {name}")).clicked()
                {
                    act = Some((i, None));
                }
                let down = ui.add_enabled_ui(i + 1 < n, |ui| {
                    widgets::small_icon_button(ui, Icon::MoveDown, &format!("Move {name} down"))
                });
                if down.inner.clicked() {
                    act = Some((i, Some(i + 1)));
                }
                let up = ui.add_enabled_ui(i > 0, |ui| {
                    widgets::small_icon_button(ui, Icon::MoveUp, &format!("Move {name} up"))
                });
                if up.inner.clicked() {
                    act = Some((i, Some(i.saturating_sub(1))));
                }
            });
        });
    }
    match act {
        Some((i, None)) if i < n => {
            list.languages.remove(i);
        }
        Some((i, Some(j))) if i < n && j < n => list.languages.swap(i, j),
        _ => {}
    }
    let left = addable(&ready, &list.languages);
    if list.languages.len() < WORD_LANGUAGES && !left.is_empty() {
        ui.menu_button("Add language", |ui| {
            for lang in left {
                let mut name = mareader_dict::language(&lang);
                if !ready.contains(&lang) {
                    name.push_str(" · not downloaded");
                }
                if ui.button(name).clicked() {
                    list.languages.push(lang);
                    ui.close();
                }
            }
        });
    }
    if list.languages.is_empty() {
        ui.label(RichText::new("No languages: the list holds the words alone.").color(t.muted));
    }
    if ready.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Download a dictionary to add meanings.").color(t.muted));
            if ui.link("Dictionary settings").clicked() {
                out.push(Sent::Engine(Command::OpenSettings(Tab::Dictionary)));
                *close = true;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_list_can_add_the_ai_dictionarys_languages_after_the_packs() {
        let ready = ["my", "russian"].map(str::to_owned);
        let listed = ["my", "fr"].map(str::to_owned);
        let left = addable(&ready, &listed);
        assert!(!left.iter().any(|l| l == "my" || l == "fr"), "listed ones");
        assert_eq!(left.last().map(String::as_str), Some("russian"));
    }
}
