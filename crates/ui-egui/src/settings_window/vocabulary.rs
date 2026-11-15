//! The Vocabulary tab: word data, shading, and how words sound.

use egui::{RichText, Slider, Ui};
use mareader_cefr::{DATASET, Part, State, TAGGER};
use mareader_settings::{
    Accent, CefrLevel, CefrMark, DictionarySettings, Notation, Pen, PronunciationSettings,
    StudySettings,
};
use mareader_speech::Stage;

use super::downloads::{Buttons, muted, progress};
use super::highlights::{palette, preview};
use super::{choices, divider, flagged_row, group, note, row, section, switch_row};
use crate::dict::{count, megabytes};
use crate::icons::Icon;
use crate::sounds::{EnglishVoice, Sounds, Spoken};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::vocab::Vocab;
use mareader_engine::command::Command;
use mareader_engine::dict::Download;

pub fn tab(
    ui: &mut Ui,
    (study, dictionary): (&mut StudySettings, &mut DictionarySettings),
    (vocab, sounds, english): (&Vocab, &Sounds, Option<&EnglishVoice>),
    out: &mut Commands,
) {
    section(ui, "Word data");
    group(ui, |ui| {
        data_row(ui, vocab, out);
        divider(ui);
        sounds_row(ui, sounds, out);
    });
    // Shading needs the levels: its options wait for them.
    let ready = *vocab.state(Part::Dataset) == State::Ready;
    if !vocab.attached() {
        note(ui, "Downloads need the desktop app.");
    } else if !ready {
        note(
            ui,
            "Download it to shade words above your level. It works offline, \
             and a paused download resumes where it stopped.",
        );
    }
    if ready {
        shading(ui, study, dictionary);
    }
    if sounds.ready() {
        section(ui, "Pronunciation");
        group(ui, |ui| {
            pronunciation(ui, &mut study.pronunciation, english)
        });
    }
}

/// Shading words above the reader's level, and how it looks.
fn shading(ui: &mut Ui, study: &mut StudySettings, dictionary: &mut DictionarySettings) {
    section(ui, "Vocabulary highlighter");
    group(ui, |ui| {
        let c = &mut study.cefr;
        let hint = "Words harder than your level, shaded in PDFs and books";
        switch_row(ui, "Shade words above my level", hint, &mut c.enabled);
        divider(ui);
        level_row(ui, &mut c.level);
        divider(ui);
        let hint = "Opens it in the AI Dictionary, marking it";
        switch_row(
            ui,
            "Click a shaded word to look it up",
            hint,
            &mut c.click_explain,
        );
        divider(ui);
        let hint = "Its dictionary card, when a language pack is downloaded";
        switch_row(
            ui,
            "Rest on a shaded word to look it up",
            hint,
            &mut dictionary.hover,
        );
    });
    section(ui, "Shading look");
    group(ui, |ui| look_rows(ui, study));
    note(
        ui,
        "Custom shading keeps its own pen and colour, distinct from highlights.",
    );
}

/// Whether the cards show how words sound, which accent, written how.
fn pronunciation(ui: &mut Ui, p: &mut PronunciationSettings, english: Option<&EnglishVoice>) {
    let hint = "Under the word on dictionary and AI cards";
    switch_row(ui, "Show on word cards", hint, &mut p.shown);
    divider(ui);
    let voice = crate::sounds::spoken(english.map(|v| v.tag.as_str()));
    let (detail, tip, warn) = following(p.follow_voice, voice, english);
    let t = Tokens::get(ui.ctx());
    let flag = warn.then(|| (Icon::Alert, crate::theme::caution(&t)));
    let title = "Match the reading voice";
    flagged_row(ui, (title, &detail), &tip, flag, |ui| {
        crate::widgets::switch(ui, &mut p.follow_voice, title);
    });
    divider(ui);
    // While the voice decides, the picker shows its choice, greyed.
    let mut accent = crate::sounds::accent(p, voice);
    let hint = match p.follow_voice {
        true => "The reading voice picks it; turn off Match the reading voice to choose",
        false => "Which English the cards say",
    };
    row(ui, "Accent", hint, |ui| {
        ui.add_enabled_ui(!p.follow_voice, |ui| {
            choices(ui, &mut accent, &Accent::ALL, Accent::label);
        });
    });
    if !p.follow_voice {
        p.accent = accent;
    }
    divider(ui);
    row(ui, "Written as", p.notation.detail(), |ui| {
        choices(ui, &mut p.notation, &Notation::ALL, Notation::label);
    });
}

/// The match row's detail and hover tip, and whether it warns.
fn following(on: bool, voice: Spoken, english: Option<&EnglishVoice>) -> (String, String, bool) {
    let name = english.map_or("Your voice", |v| v.name.as_str());
    let tag = english.map_or("", |v| v.tag.as_str());
    let named = crate::sounds::named;
    match (on, voice) {
        (false, _) => (
            "Cards use the accent you pick below".into(),
            "Turn on to show the English the reading voice speaks".into(),
            false,
        ),
        (true, Spoken::Exactly(d)) => (
            format!("{name} speaks {} English, so cards do too", named(d)),
            format!("Cards follow the voice picked in Read aloud ({tag})"),
            false,
        ),
        (true, Spoken::Nearest { dialect, english }) => (
            format!(
                "{name} speaks {english} English · showing {}, the nearest",
                named(dialect)
            ),
            crate::sounds::stand_in(english, dialect),
            true,
        ),
        (true, Spoken::Unsure) if english.is_some() => (
            format!("{name} doesn't say which English, so cards show both"),
            format!("Its voice is tagged {tag}, with no region"),
            true,
        ),
        (true, Spoken::Unsure) => (
            "The reading voice doesn't speak English, so cards show both".into(),
            "Pick an English voice in Read aloud to match it".into(),
            false,
        ),
    }
}

/// Both files as the one download the reader sees.
#[derive(Clone, Debug, PartialEq)]
enum Whole<'a> {
    Absent,
    Fetching {
        got: u64,
        total: u64,
    },
    Paused {
        got: u64,
        total: u64,
    },
    Preparing,
    /// The levels are in; the roles model is still to come.
    Partial,
    Ready,
    Failed(&'a str),
}

/// How the levels and the roles model stand together.
fn whole<'a>(levels: &'a State, roles: &'a State) -> Whole<'a> {
    let parts = [(levels, DATASET.bytes), (roles, TAGGER.bytes)];
    let size = |s: &State, guess: u64| match s {
        State::Fetching { got, total } | State::Paused { got, total } => {
            (*got, total.unwrap_or(guess))
        }
        State::Ready | State::Converting => (guess, guess),
        State::Absent | State::Failed(_) => (0, guess),
    };
    let (got, total) = parts.iter().fold((0, 0), |(g, t), (s, guess)| {
        let (a, b) = size(s, *guess);
        (g + a, t + b)
    });
    let any = |f: fn(&State) -> bool| parts.iter().any(|(s, _)| f(s));
    let failed = parts.iter().find_map(|(s, _)| match s {
        State::Failed(why) => Some(why.as_str()),
        _ => None,
    });
    if let Some(why) = failed {
        Whole::Failed(why)
    } else if any(|s| matches!(s, State::Fetching { .. })) {
        Whole::Fetching { got, total }
    } else if any(|s| matches!(s, State::Paused { .. })) {
        Whole::Paused { got, total }
    } else if any(|s| *s == State::Converting) {
        Whole::Preparing
    } else {
        match (levels, roles) {
            (State::Ready, State::Ready) => Whole::Ready,
            (State::Ready, _) => Whole::Partial,
            _ => Whole::Absent,
        }
    }
}

/// A slider over the five levels; it applies as it moves.
fn level_row(ui: &mut Ui, level: &mut CefrLevel) {
    let all = CefrLevel::ALL;
    let mut at = all.iter().position(|l| l == level).unwrap_or(2);
    row(ui, "My level", level.detail(), |ui| {
        let t = Tokens::get(ui.ctx());
        ui.label(RichText::new(level.label()).strong().color(t.ink));
        ui.add_space(t.gap);
        let max = all.len().saturating_sub(1);
        let slider = Slider::new(&mut at, 0..=max).show_value(false);
        let r = ui.add_sized([150.0, 20.0], slider);
        r.widget_info(|| {
            let mut info =
                egui::WidgetInfo::labeled(egui::WidgetType::Slider, true, "Reading level");
            info.value = Some(at as f64);
            info
        });
    });
    *level = all.get(at).copied().unwrap_or_default();
}

/// The shading's pen and colour, which highlights then can't take.
fn look_rows(ui: &mut Ui, study: &mut StudySettings) {
    row(
        ui,
        "Style",
        "Level colours and weight, or your original custom pen",
        |ui| {
            choices(ui, &mut study.cefr.mark, &CefrMark::ALL, CefrMark::label);
        },
    );
    if study.cefr.mark != CefrMark::Custom {
        return;
    }
    divider(ui);
    let h = &mut study.highlights;
    let mut cefr = h.cefr;
    row(ui, "Mark", "How hard words are shaded", |ui| {
        choices(ui, &mut cefr.pen, &Pen::ALL, Pen::label);
        preview(ui, cefr.pen, cefr.color, h.opacity);
    });
    divider(ui);
    let (taken, ai) = (h.color(cefr.pen), h.ai);
    row(
        ui,
        "Colour",
        "A pen's own colour and the AI's look can't be used",
        |ui| {
            let kept = "Kept for a pen or for AI marks";
            let free = |c| c != taken && !(ai.pen == cefr.pen && ai.color == c);
            palette(ui, ("Shading", kept), &mut cefr.color, free);
        },
    );
    h.cefr = cefr;
}

/// The levels and the roles model, downloaded, paused and removed as one.
fn data_row(ui: &mut Ui, vocab: &Vocab, out: &mut Commands) {
    let data = Buttons {
        name: "word data",
        ask: |what| Sent::Engine(Command::Vocab(what)),
    };
    let hint = "Each word's CEFR level, and a model of its role in a sentence";
    let all = DATASET.bytes + TAGGER.bytes;
    row(ui, "Word data", hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !vocab.attached() {
            muted(ui, &megabytes(all));
            return;
        }
        let levels = vocab.state(Part::Dataset);
        match whole(levels, vocab.state(Part::Tagger)) {
            Whole::Absent => {
                data.press(ui, (Icon::Download, "Download"), Download::Start, out);
                muted(ui, &megabytes(all));
            }
            Whole::Fetching { got, total } => {
                data.press(ui, (Icon::Close, "Cancel"), Download::Cancel, out);
                data.press(ui, (Icon::Pause, "Pause"), Download::Pause, out);
                progress(ui, got, total);
            }
            Whole::Paused { got, .. } => data.paused(ui, got, out),
            Whole::Preparing => {
                ui.spinner();
                muted(ui, "Preparing…");
            }
            Whole::Partial => {
                data.remove(ui, out);
                // The roles came later: shading works, a click's role waits.
                data.press(ui, (Icon::Download, "Download"), Download::Start, out);
                muted(ui, "Word roles missing");
                words(ui, vocab);
            }
            Whole::Ready => {
                data.remove(ui, out);
                words(ui, vocab);
            }
            Whole::Failed(why) => data.failed(ui, why, out),
        }
    });
}

/// The pronunciation pack; one a voice model holds is in already.
fn sounds_row(ui: &mut Ui, sounds: &Sounds, out: &mut Commands) {
    let pack = Buttons {
        name: "pronunciation",
        ask: |what| Sent::Engine(Command::Pronunciation(what)),
    };
    let hint = "How words sound, American and British, shared with voice models";
    let size = sounds.bytes();
    row(ui, "Pronunciation", hint, |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if !sounds.attached() {
            muted(ui, &megabytes(size));
            return;
        }
        let stage = sounds.stage();
        if *stage != Stage::Ready {
            pack.controls(ui, stage, size, out);
            return;
        }
        match sounds.shared_with().first() {
            // Its files are a voice model's too: removing frees nothing.
            Some(name) => {
                muted(ui, &format!("Shared with {name}"));
            }
            None => {
                pack.remove(ui, out);
                muted(ui, &megabytes(size));
            }
        }
    });
}

fn words(ui: &mut Ui, vocab: &Vocab) {
    let words = vocab.words().map(|n| format!("{} words", count(n)));
    muted(ui, words.as_deref().unwrap_or("Downloaded"));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_files_read_as_one_download() {
        let fetching = |got| State::Fetching {
            got,
            total: Some(1_000),
        };
        let (a, b) = (fetching(250), fetching(500));
        let both = whole(&a, &b);
        let (got, total) = (750, 2_000);
        assert_eq!(both, Whole::Fetching { got, total });
        let levels = State::Ready;
        let ready = whole(&levels, &b);
        let got = DATASET.bytes + 500;
        let total = DATASET.bytes + 1_000;
        assert_eq!(ready, Whole::Fetching { got, total });
        let why = State::Failed("offline".into());
        assert_eq!(whole(&State::Ready, &why), Whole::Failed("offline"));
        assert_eq!(whole(&State::Ready, &State::Absent), Whole::Partial);
        assert_eq!(whole(&State::Ready, &State::Ready), Whole::Ready);
        assert_eq!(whole(&State::Absent, &State::Absent), Whole::Absent);
    }
}
