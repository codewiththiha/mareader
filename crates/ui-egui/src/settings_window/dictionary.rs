//! The Dictionary tab: language packs to download, and the card's
//! language.

use egui::Ui;
use mareader_dict::{HUB, PACKS, Pack, State, language, languages};
use mareader_settings::{DictionaryOpens, DictionarySettings};

use super::downloads::{Buttons, muted, progress};
use super::{
    choice_row, divider, fold_open, fold_row, folded, group, note, row, section, switch_row,
};
use crate::dict::{count, megabytes};
use crate::icons::Icon;
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::dict::{Action, Download, Service as DictService};

/// The fold of packs not on this device, beside some that are.
pub const MORE_LANGUAGES: &str = "More languages";
/// The same fold while no pack is on this device.
pub const ALL_LANGUAGES: &str = "Available languages";

pub fn tab(ui: &mut Ui, s: &mut DictionarySettings, dict: &DictService, out: &mut Commands) {
    section(ui, "Language packs");
    let mut packs: Vec<&'static Pack> = PACKS.iter().collect();
    packs.sort_by_key(|p| language(p.target));
    // Downloading counts as here, so its progress never hides in the fold.
    let (here, more): (Vec<_>, Vec<_>) =
        (packs.into_iter()).partition(|p| dict.attached() && *dict.state(p) != State::Absent);
    group(ui, |ui| {
        for (i, &pack) in here.iter().enumerate() {
            if i > 0 {
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
    if dict.attached() {
        note(
            ui,
            "Packs work offline once downloaded. A paused download resumes where it stopped.",
        );
    } else {
        note(ui, "Downloads need the desktop app.");
    }
    section(ui, "In the reader");
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
    note(ui, "The Dictionary button and Cmd/Ctrl+Shift+D open it so.");
    section(ui, "Lookup card");
    group(ui, |ui| {
        default_language(ui, s, &dict.ready());
        divider(ui);
        let hint = "With no entry for a word, the closest ones in its place";
        switch_row(ui, "Show near matches", hint, &mut s.near);
    });
    note(
        ui,
        "The card answers in this language. Only a language whose pack is downloaded is offered.",
    );
    section(ui, "Saying words");
    group(ui, |ui| {
        let hint = "A word in another language is said by a voice downloaded for it";
        switch_row(ui, "Use other languages' voices", hint, &mut s.other_voices);
    });
    note(
        ui,
        "Only voices of the engine in use. Each may sound unlike yours.",
    );
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

fn default_language(ui: &mut Ui, s: &mut DictionarySettings, ready: &[&'static Pack]) {
    // English is what packs translate from; the card answers in another.
    let offered: Vec<&str> = languages(ready).into_iter().filter(|l| *l != HUB).collect();
    let chosen: Option<&str> = s
        .default_lang
        .as_deref()
        .and_then(|l| offered.iter().copied().find(|o| *o == l));
    let shown = chosen.map_or("Automatic".to_owned(), language);
    row(
        ui,
        "Default language",
        "What the card translates into",
        |ui| {
            ui.add_enabled_ui(!offered.is_empty(), |ui| {
                widgets::combo("dict-default-lang")
                    .width(180.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(chosen.is_none(), "Automatic").clicked() {
                            s.default_lang = None;
                        }
                        for l in &offered {
                            if ui
                                .selectable_label(chosen == Some(*l), language(l))
                                .clicked()
                            {
                                s.default_lang = Some((*l).to_owned());
                            }
                        }
                    });
            });
        },
    );
}
