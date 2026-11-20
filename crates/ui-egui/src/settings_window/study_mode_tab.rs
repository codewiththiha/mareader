//! The Study mode tab: the switch, batches, timer and card source.

use egui::Ui;
use mareader_settings::StudyModeSettings;
use mareader_study::{BatchUnit, Mode, Source};

use super::{divider, group, note, row, section, switch_row};
use crate::ui_command::{Commands, Sent};
use crate::widgets;
use mareader_engine::command::Command;
use mareader_engine::study_mode::Action;

pub fn tab(ui: &mut Ui, mode: &mut StudyModeSettings, out: &mut Commands) {
    mode_section(ui, mode, out);
    batches_section(ui, mode);
    proactive_section(ui, mode);
    reactive_section(ui, mode);
    note(
        ui,
        "Answer in any language; the judge checks meaning, not spelling.",
    );
}

fn mode_section(ui: &mut Ui, mode: &mut StudyModeSettings, out: &mut Commands) {
    section(ui, "Mode");
    group(ui, |ui| {
        row(ui, "Mode", "Also on the page's right-click menu", |ui| {
            let before = mode.mode;
            ui.selectable_value(&mut mode.mode, Mode::Off, "Off");
            ui.selectable_value(&mut mode.mode, Mode::Proactive, "Proactive");
            ui.selectable_value(&mut mode.mode, Mode::Reactive, "Reactive");
            if before != mode.mode {
                out.push(Sent::Engine(Command::StudyMode(Action::Set(mode.mode))));
            }
        });
        divider(ui);
        row(ui, "My level", "Words above this become cards", |ui| {
            let level = &mut mode.dials.level;
            for (band, name) in [(2u8, "A2"), (3, "B1"), (4, "B2"), (5, "C1"), (6, "C2")] {
                ui.selectable_value(level, band, name);
            }
        });
    });
}

fn batches_section(ui: &mut Ui, mode: &mut StudyModeSettings) {
    section(ui, "Batches");
    group(ui, |ui| {
        row(
            ui,
            "Size by",
            "Pages you see, or a character budget for EPUBs",
            |ui| {
                let unit = &mut mode.dials.unit;
                ui.selectable_value(unit, BatchUnit::Pages, "Pages");
                ui.selectable_value(unit, BatchUnit::Chars, "Characters");
            },
        );
        divider(ui);
        if mode.dials.unit == BatchUnit::Pages {
            row(ui, "Pages per batch", "Most is 10", |ui| {
                let mut v = mode.dials.pages as f32;
                widgets::stepper(ui, &mut v, 1.0..=10.0, 1.0, |x| format!("{x:.0}"), "pages");
                mode.dials.pages = v.round() as usize;
            });
        } else {
            row(
                ui,
                "Characters per batch",
                "Rounded to whole pages, never over 10",
                |ui| {
                    let mut v = mode.dials.chars as f32;
                    widgets::stepper(
                        ui,
                        &mut v,
                        500.0..=8_000.0,
                        100.0,
                        |x| format!("{x:.0}"),
                        "characters",
                    );
                    mode.dials.chars = v.round() as usize;
                },
            );
        }
    });
}

fn proactive_section(ui: &mut Ui, mode: &mut StudyModeSettings) {
    section(ui, "Proactive");
    group(ui, |ui| {
        row(ui, "Prepare ahead", "Decks written while you read", |ui| {
            let mut v = mode.dials.ahead as f32;
            widgets::stepper(ui, &mut v, 0.0..=3.0, 1.0, |x| format!("{x:.0}"), "batches");
            mode.dials.ahead = v.round() as usize;
        });
        divider(ui);
        row(
            ui,
            "Study time per word",
            "Added up across the deck",
            |ui| {
                let mut v = mode.dials.sec_per_card as f32;
                widgets::stepper(
                    ui,
                    &mut v,
                    2.0..=20.0,
                    1.0,
                    |x| format!("{x:.0}s"),
                    "seconds",
                );
                mode.dials.sec_per_card = v.round() as u32;
            },
        );
        divider(ui);
        switch_row(
            ui,
            "Strict timer",
            "On: Start reading waits for the timer",
            &mut mode.dials.strict,
        );
        divider(ui);
        switch_row(
            ui,
            "Shade hard words while reading",
            "Off by default so Proactive is a real test",
            &mut mode.dials.hints,
        );
        divider(ui);
        row(
            ui,
            "Words per deck",
            "Hardest and most repeated first",
            |ui| {
                let mut v = mode.dials.max_cards as f32;
                widgets::stepper(ui, &mut v, 4.0..=30.0, 1.0, |x| format!("{x:.0}"), "cards");
                mode.dials.max_cards = v.round() as usize;
            },
        );
        divider(ui);
        row(
            ui,
            "Cards from",
            "Auto uses the AI, then the offline pack",
            |ui| {
                let source = &mut mode.dials.source;
                ui.selectable_value(source, Source::Auto, "Auto");
                ui.selectable_value(source, Source::Ai, "AI");
                ui.selectable_value(source, Source::Dict, "Dictionary");
            },
        );
    });
}

fn reactive_section(ui: &mut Ui, mode: &mut StudyModeSettings) {
    section(ui, "Reactive");
    group(ui, |ui| {
        switch_row(
            ui,
            "Ask before adding to memorized",
            "After a correct answer; off memorizes at once",
            &mut mode.dials.ask_memorize,
        );
    });
}
