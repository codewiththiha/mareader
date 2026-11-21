//! The Study mode tab of the settings window: on or off, proactive or
//! reactive, and the budgets a batch and its deck are built from.

use egui::{DragValue, Ui};
use mareader_study::{BatchCfg, BatchUnit, CardSource, StudyMode, StudyModeSettings};

use super::{choices, group, row, section, switch_row};

/// Draws the Study mode tab, editing `settings` in place; the window turns
/// any change into one `SetSettings` command.
pub(crate) fn tab(ui: &mut Ui, settings: &mut StudyModeSettings) {
    mode_section(ui, settings);
    budgets_section(ui, settings);
    rules_section(ui, settings);
}

/// The mode, source and unit choices; the top group of the tab.
fn mode_section(ui: &mut Ui, settings: &mut StudyModeSettings) {
    section(ui, "Study mode");
    group(ui, |ui| {
        row(
            ui,
            "Mode",
            "Off, proactive (learn before reading) or reactive (recall as you meet each word).",
            |ui| {
                choices(ui, &mut settings.mode, &StudyMode::ALL, StudyMode::label);
            },
        );
        row(
            ui,
            "Cards come from",
            "The study task's model, else the dictionary pack; Auto picks whichever resolves.",
            |ui| {
                choices(
                    ui,
                    &mut settings.source,
                    &CardSource::ALL,
                    CardSource::label,
                );
            },
        );
        row(
            ui,
            "Batch unit",
            "A batch is that many pages, or that many characters.",
            |ui| {
                choices(ui, &mut settings.unit, &BatchUnit::ALL, BatchUnit::label);
            },
        );
    });
}

/// Pages, chars, ahead, seconds-per-card and cards-per-deck budgets.
fn budgets_section(ui: &mut Ui, settings: &mut StudyModeSettings) {
    section(ui, "Budgets");
    group(ui, |ui| {
        row(
            ui,
            "Pages per batch",
            "How many pages a batch gates, in pages unit.",
            |ui| {
                let mut v = settings.pages as f32;
                ui.add(DragValue::new(&mut v).speed(1.0).suffix(" pages"));
                settings.pages =
                    v.clamp(BatchCfg::PAGES_MIN as f32, BatchCfg::PAGES_MAX as f32) as usize;
            },
        );
        row(
            ui,
            "Chars per batch",
            "How many characters a batch gates, in chars unit.",
            |ui| {
                let mut v = settings.chars as f32;
                ui.add(DragValue::new(&mut v).speed(50.0).suffix(" chars"));
                settings.chars =
                    v.clamp(BatchCfg::CHARS_MIN as f32, BatchCfg::CHARS_MAX as f32) as usize;
            },
        );
        row(
            ui,
            "Decks ahead",
            "How many coming batches have decks prepared while you read. 0 waits for the lock.",
            |ui| {
                let mut v = settings.ahead as f32;
                ui.add(DragValue::new(&mut v).speed(1.0));
                settings.ahead = v.clamp(
                    StudyModeSettings::AHEAD_MIN as f32,
                    StudyModeSettings::AHEAD_MAX as f32,
                ) as usize;
            },
        );
        row(
            ui,
            "Seconds per card",
            "The floor timer's share of each card; strict adds them up.",
            |ui| {
                let mut v = settings.sec_per_card as f32;
                ui.add(DragValue::new(&mut v).speed(1.0).suffix(" s"));
                settings.sec_per_card = v.clamp(
                    StudyModeSettings::SEC_PER_CARD_MIN as f32,
                    StudyModeSettings::SEC_PER_CARD_MAX as f32,
                ) as u8;
            },
        );
        row(
            ui,
            "Cards per deck",
            "How many words a deck holds, ranked and capped.",
            |ui| {
                let mut v = settings.max_cards as f32;
                ui.add(DragValue::new(&mut v).speed(1.0));
                settings.max_cards = v.clamp(
                    StudyModeSettings::MAX_CARDS_MIN as f32,
                    StudyModeSettings::MAX_CARDS_MAX as f32,
                ) as usize;
            },
        );
    });
}

/// Strict timer, shade and ask-memorize switches; the rules group.
fn rules_section(ui: &mut Ui, settings: &mut StudyModeSettings) {
    section(ui, "Rules");
    group(ui, |ui| {
        switch_row(
            ui,
            "Strict timer",
            "The floor timer must elapse before a batch unlocks.",
            &mut settings.strict,
        );
        switch_row(
            ui,
            "Shade hard words",
            "Even proactive highlights words above the reader's level as they come up.",
            &mut settings.hints,
        );
        switch_row(
            ui,
            "Ask before memorized",
            "A correct answer still asks once before the word is taken as known.",
            &mut settings.ask_memorize,
        );
    });
}
