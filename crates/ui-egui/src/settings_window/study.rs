//! The Study mode tab: the mode the page menu turns on, batch sizes, the
//! deck before each batch, the recall, and where the words are kept.

use egui::{Button, RichText, Ui};
use mareader_settings::{CefrLevel, Settings, StudyModeSettings};
use mareader_study::mode::{MAX_PAGES, Mode, Source, Unit};

use super::{choices, divider, group, row, section, section_help, switch_row};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use crate::widgets;
use mareader_engine::command::Command;

/// The button that leaves for Space, for screen readers and tests.
pub const OPEN_VOCABS: &str = "Open Vocabs";
/// Under Reactive: what the judge reads an answer for.
const ANY_LANGUAGE: &str = "Answers may be in any language: the AI judges meaning, not spelling.";
/// A count's unit for one, then for more.
const BATCHES: (&str, &str) = (" batch", " batches");
const SECONDS: (&str, &str) = (" s", " s");
const BARE: (&str, &str) = ("", "");

/// The tab, given who could write the cards: an AI, a dictionary.
pub fn tab(
    ui: &mut Ui,
    s: &mut Settings,
    (ai, dict): (bool, bool),
    #[cfg(not(target_arch = "wasm32"))] board: Option<&mareader_assist::Board>,
    out: &mut Commands,
) {
    let level = level(s.study.cefr.level);
    section_help(ui, "Mode", &[("", &level)]);
    group(ui, |ui| {
        let hint = "The one the page menu's Study switch turns on";
        let m = &mut s.study.mode;
        row(ui, "Mode", hint, |ui| {
            choices(ui, &mut m.mode, &Mode::ALL, Mode::label);
        });
    });
    batches(ui, &mut s.study.mode);
    proactive(ui, &mut s.study.mode, ai && dict);
    section_help(ui, "Reactive", &[("", ANY_LANGUAGE)]);
    group(ui, |ui| {
        let hint = "After a correct answer; off memorizes it at once";
        let ask = &mut s.study.mode.ask_memorize;
        switch_row(ui, "Ask before adding to memorized", hint, ask);
    });
    #[cfg(not(target_arch = "wasm32"))]
    model(ui, &mut s.ai, board);
    space(ui, out);
}

/// Under the mode: the level is Vocabulary's, shared rather than set here.
fn level(level: CefrLevel) -> String {
    let at = level.label();
    format!("Uses your Vocabulary level ({at}): words above it are shaded or become cards.")
}

/// How big a batch is: whole pages holding about a deck's words, or a
/// set number of pages; either way a deck holds every word on them.
fn batches(ui: &mut Ui, m: &mut StudyModeSettings) {
    section(ui, "Batches");
    group(ui, |ui| {
        let hint = "Words gathers whole pages until they hold about a deck";
        row(ui, "Size by", hint, |ui| {
            choices(ui, &mut m.unit, &[Unit::Words, Unit::Pages], unit);
        });
        if m.unit == Unit::Pages {
            divider(ui);
            let most = u8::try_from(MAX_PAGES).unwrap_or(u8::MAX);
            let hint = "Most is 10";
            count_row(ui, ("Pages per batch", hint), &mut m.pages, (1, most), BARE);
        }
        divider(ui);
        let hint = "The cards a batch aims at; words mode breaks pages near it";
        let cards = (StudyModeSettings::CARDS_MIN, StudyModeSettings::CARDS_MAX);
        count_row(ui, ("Words per deck", hint), &mut m.max_cards, cards, BARE);
    });
}

/// The deck before each batch: made ahead, its time, the lock, how many
/// cards and, when both could, by whom.
fn proactive(ui: &mut Ui, m: &mut StudyModeSettings, both: bool) {
    section(ui, "Proactive");
    group(ui, |ui| {
        let hint = "Decks made while you read; 0 waits for the gate";
        let ahead = (0, StudyModeSettings::AHEAD_MAX);
        count_row(ui, ("Prepare ahead", hint), &mut m.ahead, ahead, BATCHES);
        divider(ui);
        let hint = "Off lets a deck stay open, and open again, as long as you like";
        switch_row(ui, "Time limit on cards", hint, &mut m.limit);
        if m.limit {
            divider(ui);
            let full = f64::from(m.card_seconds) * f64::from(m.max_cards);
            let time = crate::study_mode::deck::clock(full);
            let hint = format!("Counted while cards show; {} cards: {time}", m.max_cards);
            let range = (
                StudyModeSettings::CARD_SECONDS_MIN,
                StudyModeSettings::CARD_SECONDS_MAX,
            );
            let n = &mut m.card_seconds;
            count_row(ui, ("Seconds per card", &hint), n, range, SECONDS);
        }
        divider(ui);
        let hint = "Pales the pages before where study mode turned on";
        switch_row(ui, "Lock pages outside the batches", hint, &mut m.lock);
        divider(ui);
        let hint = "Off makes it a real test";
        switch_row(ui, "Shade hard words while reading", hint, &mut m.hints);
        if both {
            divider(ui);
            let hint = "Auto asks the AI first, else the dictionary";
            row(ui, "Cards from", hint, |ui| {
                let all = [Source::Auto, Source::Ai, Source::Dictionary];
                choices(ui, &mut m.source, &all, source);
            });
        }
    });
}

/// Study's own models and language; each follows the AI Dictionary's unset.
/// With no model set up there is nothing to pick.
#[cfg(not(target_arch = "wasm32"))]
fn model(
    ui: &mut Ui,
    ai: &mut mareader_settings::AiSettings,
    board: Option<&mareader_assist::Board>,
) {
    use super::ai::{Home, model_row, placed};
    const SAME: &str = "Same as Dictionary";
    if ai.presets.is_empty() {
        return;
    }
    let apple = ai
        .model(mareader_settings::TaskId::StudyCards)
        .is_some_and(|p| p.provider == mareader_settings::Provider::Apple);
    let state = board.and_then(|b| b.apple.ready()).filter(|_| apple);
    let speaks = |tag: &str| state.and_then(|s| s.speaks(tag));
    section(ui, "AI");
    group(ui, |ui| {
        for task in placed(Home::Study) {
            model_row(ui, ai, task);
            divider(ui);
        }
        let hint = "AI decks write meanings and feedback in it; synonyms and examples stay \
                    in English. Same as Dictionary uses its default, or English";
        row(ui, "Cards in", hint, |ui| {
            let language = &mut ai.tasks.study.language;
            super::ai::language_picker(ui, ("study-language", SAME), language, &speaks);
        });
    });
}

/// The row that leaves for Space's Vocabs, where every studied word is kept.
fn space(ui: &mut Ui, out: &mut Commands) {
    section(ui, "Your words");
    group(ui, |ui| {
        let hint = "Every word studied, in every book";
        row(ui, "Vocabs", hint, |ui| {
            let t = Tokens::get(ui.ctx());
            let text = RichText::new(OPEN_VOCABS).color(t.accent);
            if ui.add(Button::new(text).fill(t.accent_soft)).clicked() {
                out.push(Sent::Ui(UiCommand::CloseSettings));
                let words = mareader_engine::space::Room::Words;
                out.push(Sent::Engine(Command::ShowSpace(words)));
            }
        });
    });
}

/// A count stepped between `lo` and `hi`, shown with its unit for one or more.
pub(super) fn count_row(
    ui: &mut Ui,
    (label, hint): (&str, &str),
    n: &mut u8,
    (lo, hi): (u8, u8),
    (one, many): (&str, &str),
) {
    row(ui, label, hint, |ui| {
        let mut value = f32::from(*n);
        let range = f32::from(lo)..=f32::from(hi);
        let show = |v: f32| {
            let unit = if (v - 1.0).abs() < 0.5 { one } else { many };
            format!("{v:.0}{unit}")
        };
        widgets::stepper(ui, &mut value, range, 1.0, show, label);
        *n = value.round().clamp(f32::from(lo), f32::from(hi)) as u8;
    });
}

fn unit(unit: Unit) -> &'static str {
    match unit {
        Unit::Words => "Words",
        Unit::Pages => "Pages",
    }
}

fn source(source: Source) -> &'static str {
    match source {
        Source::Auto => "Auto",
        Source::Ai => "AI",
        Source::Dictionary => "Dictionary",
    }
}
