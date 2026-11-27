//! The Dictionary tab: language packs to download, the AI dictionary's
//! languages, and the card's language.

use egui::{RichText, Ui};
use mareader_dict::{PACKS, Pack, State, language};
use mareader_settings::{DictionaryOpens, DictionarySettings};

use super::downloads::{Buttons, muted, progress};
use super::{
    choice_row, divider, fold_open, fold_row, folded, group, note, row, section_help, switch_row,
};
use crate::dict::{count, megabytes};
use crate::icons::Icon;
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Download, Service as DictService};

/// Where the languages AI writes in are picked.
pub const ELSEWHERE: &str = "AI writes Explain's meanings in AI › Meaning in, and AI \
                             decks in Study mode › Cards in.";
/// The fold of packs not on this device, beside some that are.
pub const MORE_LANGUAGES: &str = "More languages";
/// The fold for packs already on this device.
pub const DOWNLOADED_LANGUAGES: &str = "Downloaded languages";
/// The same fold while no pack is on this device.
pub const ALL_LANGUAGES: &str = "Available languages";

pub fn tab(ui: &mut Ui, s: &mut DictionarySettings, dict: &DictService, out: &mut Commands) {
    section_help(
        ui,
        "Language packs",
        &[(
            "",
            "Packs work offline once downloaded. A paused download resumes where it stopped.",
        )],
    );
    packs(ui, dict, out);
    if !dict.attached() {
        note(ui, "Downloads need the desktop app.");
    }
    ai_dictionary(ui, dict, out);
    section_help(
        ui,
        "In the reader",
        &[("", "The Dictionary button and Cmd/Ctrl+Shift+D open it so.")],
    );
    group(ui, |ui| {
        for (i, (choice, title, detail)) in OPENS.into_iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            if choice_row(ui, title, detail, s.opens == choice) {
                s.opens = choice;
            }
        }
    });
    lookup_card(ui, s, dict);
    saying(ui, s);
}

fn packs(ui: &mut Ui, dict: &DictService, out: &mut Commands) {
    let mut packs: Vec<&'static Pack> = PACKS.iter().collect();
    packs.sort_by_key(|p| language(p.target));
    // Downloading counts as here, so its progress never hides in the fold.
    let (here, more): (Vec<&'static Pack>, Vec<&'static Pack>) =
        (packs.into_iter()).partition(|p| dict.attached() && *dict.state(p) != State::Absent);
    let (ready, active): (Vec<&'static Pack>, Vec<&'static Pack>) = here
        .iter()
        .copied()
        .partition(|pack| matches!(dict.state(pack), State::Ready));
    group(ui, |ui| {
        if !ready.is_empty() {
            let id = egui::Id::new("dict-downloaded-languages");
            if ui.data(|d| d.get_temp::<bool>(id)).is_none() {
                ui.data_mut(|d| d.insert_temp(id, true));
            }
            let detail = format!("{} downloaded", ready.len());
            if fold_row(ui, id, (DOWNLOADED_LANGUAGES, &detail)) {
                folded(ui, |ui| {
                    for &pack in &ready {
                        divider(ui);
                        pack_row(ui, pack, dict, out);
                    }
                });
            }
        }
        for (i, &pack) in active.iter().enumerate() {
            if i > 0 || !ready.is_empty() {
                divider(ui);
            }
            pack_row(ui, pack, dict, out);
        }
        if more.is_empty() {
            return;
        }
        if !here.is_empty() {
            divider(ui);
        }
        let id = egui::Id::new("dict-more-languages");
        // With nothing here yet, the choice is the point of the tab.
        if here.is_empty() && ui.data(|d| d.get_temp::<bool>(id)).is_none() {
            ui.data_mut(|d| d.insert_temp(id, true));
        }
        let title = match here.is_empty() {
            true => ALL_LANGUAGES,
            false => MORE_LANGUAGES,
        };
        // Open, the rows name them; folded, the line below does.
        let detail = match fold_open(ui.ctx(), id) {
            true => format!("{} ready to download", more.len()),
            false => (more.iter().map(|p| language(p.target)))
                .collect::<Vec<_>>()
                .join(", "),
        };
        if !fold_row(ui, id, (title, &detail)) {
            return;
        }
        folded(ui, |ui| {
            for &pack in &more {
                divider(ui);
                pack_row(ui, pack, dict, out);
            }
        });
    });
}

/// The AI dictionary's languages, each cleared on a second press.
fn ai_dictionary(ui: &mut Ui, dict: &DictService, out: &mut Commands) {
    let mut langs = dict.ai_languages().to_vec();
    if langs.is_empty() {
        return;
    }
    langs.sort_by_key(|l| language(l));
    let why = "Meanings AI wrote on this computer, by language. Clearing one deletes \
               them; cards and decks then ask the AI again.";
    section_help(ui, "AI dictionary", &[("", why)]);
    group(ui, |ui| {
        for (i, lang) in langs.iter().enumerate() {
            if i > 0 {
                divider(ui);
            }
            ai_row(ui, lang, out);
        }
    });
}

/// One AI dictionary language: Clear, then Clear for good or Keep.
fn ai_row(ui: &mut Ui, lang: &str, out: &mut Commands) {
    let t = Tokens::get(ui.ctx());
    let name = language(lang);
    let id = egui::Id::new(("ai-dict-clear", lang));
    let mut asked: bool = ui.data(|d| d.get_temp(id)).unwrap_or(false);
    row(ui, &name, "Kept on this computer", |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let label = if asked { "Clear for good" } else { "Clear" };
        let r = ui.button(RichText::new(label).color(t.danger));
        widgets::name(&r, &format!("{label}: {name}"));
        if r.clicked() {
            if asked {
                let clear = Action::Clear(lang.to_owned());
                out.push(Sent::Engine(Command::Dict(clear)));
            }
            asked = !asked;
        }
        if asked && ui.button("Keep").clicked() {
            asked = false;
        }
    });
    ui.data_mut(|d| d.insert_temp(id, asked));
}

/// Whether other languages' voices say their words.
fn saying(ui: &mut Ui, s: &mut DictionarySettings) {
    section_help(
        ui,
        "Saying words",
        &[(
            "",
            "Only voices of the engine in use. Each may sound unlike yours.",
        )],
    );
    group(ui, |ui| {
        let hint = "A word in another language is said by a voice downloaded for it";
        switch_row(ui, "Use other languages' voices", hint, &mut s.other_voices);
    });
}

/// The card's language, near matches and which dictionary leads.
fn lookup_card(ui: &mut Ui, s: &mut DictionarySettings, dict: &DictService) {
    let why = "The card answers in this language. A language is offered once its pack is \
               downloaded, or once the AI dictionary keeps a meaning in it.";
    section_help(ui, "Lookup card", &[("", why), ("", ELSEWHERE)]);
    group(ui, |ui| {
        let langs = dict.languages();
        default_language(ui, s, &langs);
        divider(ui);
        let hint = "With no entry for a word, the closest ones in its place";
        switch_row(ui, "Show near matches", hint, &mut s.near);
        divider(ui);
        let hint = "Meanings AI wrote on this computer show before the packs' rows";
        switch_row(ui, "AI dictionary first", hint, &mut s.ai_first);
    });
}

/// A pack named by its language; buttons keep the full "English → X".
fn pack_row(ui: &mut Ui, pack: &'static Pack, dict: &DictService, out: &mut Commands) {
    let from = language(pack.source);
    let hint = format!("From {from} · {} words", count(pack.entries));
    let label = pack.label();
    let id = pack.id;
    let buttons = Buttons {
        name: &label,
        ask: |what| {
            Sent::Engine(Command::Dict(match what {
                Download::Start => Action::Download(id),
                Download::Pause => Action::Pause(id),
                Download::Cancel => Action::Cancel(id),
                Download::Remove => Action::Remove(id),
            }))
        },
    };
    row(ui, &language(pack.target), &hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !dict.attached() {
            muted(ui, &megabytes(pack.bytes));
            return;
        }
        match dict.state(pack) {
            State::Absent => {
                buttons.press(ui, (Icon::Download, "Download"), Download::Start, out);
                muted(ui, &megabytes(pack.bytes));
            }
            State::Fetching { got, total } => {
                buttons.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
                buttons.press(ui, (Icon::Pause, "Pause"), Download::Pause, out);
                progress(ui, *got, total.unwrap_or(pack.bytes));
            }
            State::Paused { got, .. } => buttons.paused(ui, *got, out),
            State::Converting => {
                ui.spinner();
                muted(ui, "Preparing…");
            }
            State::Ready => {
                buttons.remove(ui, out);
                muted(ui, "Downloaded");
            }
            State::Failed(why) => buttons.failed(ui, why, out),
        }
    });
}

/// How the reader's Dictionary button opens it, and what that means.
const OPENS: [(DictionaryOpens, &str, &str); 2] = [
    (
        DictionaryOpens::Window,
        "Floating window",
        "Over the page; move, resize, fold or close it.",
    ),
    (
        DictionaryOpens::Page,
        "Full page",
        "In place of the panes, as the library shows.",
    ),
];

fn default_language(ui: &mut Ui, s: &mut DictionarySettings, offered: &[String]) {
    let chosen: Option<&str> = s
        .default_lang
        .as_deref()
        .and_then(|l| offered.iter().map(String::as_str).find(|o| *o == l));
    let shown = chosen.map_or("Automatic".to_owned(), language);
    row(
        ui,
        "Default language",
        "Lookup cards, word lists and dictionary decks answer in it",
        |ui| {
            ui.add_enabled_ui(!offered.is_empty(), |ui| {
                let mut selected = chosen.map(str::to_owned);
                let before = selected.clone();
                let mut items = vec![(None, "Automatic".to_owned())];
                items.extend(offered.iter().map(|tag| (Some(tag.clone()), language(tag))));
                crate::dropdown::select(
                    ui,
                    egui::Id::new("dict-default-lang"),
                    shown,
                    180.0,
                    &mut selected,
                    &items,
                );
                if selected != before {
                    s.default_lang = selected;
                }
            });
        },
    );
}
