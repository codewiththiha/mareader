//! The proactive gate: a deck over a dimmed window until the timer allows.

use egui::{Align, Context, Layout, RichText};
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::study_mode::Action;
use mareader_settings::StudyModeSettings;
use mareader_study::Mode;

use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};
use crate::widgets;

/// Window-ephemeral deck state: which card shows, whether it is revealed.
#[derive(Default)]
pub struct ModeUi {
    card: usize,
    revealed: bool,
}

impl ModeUi {
    pub fn reset(&mut self) {
        self.card = 0;
        self.revealed = false;
    }

    /// Draws the gate when proactive has one open; true if it asked to close.
    pub fn show(
        &mut self,
        ctx: &Context,
        session: &Session,
        settings: &StudyModeSettings,
        out: &mut Commands,
    ) -> bool {
        if session.study_mode().mode() != Mode::Proactive {
            return false;
        }
        let Some(gate) = session.study_mode().gate() else {
            return false;
        };
        let range = gate.range();
        let cards = gate.deck.cards.clone();
        let left = gate.left_secs;
        let strict = settings.dials.strict;
        let found = gate.deck.found;
        let skipped = gate.deck.skipped_known;
        let mut action = None;
        let t = Tokens::get(ctx);
        let resp = Dialog::new("study-gate").show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.set_max_width(560.0);
            modal::heading(ui, None, &format!("Words for {range}"), None, &t);
            let meta = format!(
                "Above level {} · {found} found · {skipped} skipped as memorized",
                settings.dials.level
            );
            ui.label(RichText::new(meta).color(t.muted).small());
            ui.add_space(12.0);
            if cards.is_empty() {
                ui.label("Nothing left to learn here.");
            } else {
                self.card_ui(ui, &cards, &t, &mut action);
            }
            ui.add_space(14.0);
            ui.separator();
            self.footer(ui, left, strict, &t, &mut action);
        });
        if strict && left > 0 {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
        if let Some(act) = action {
            out.push(Sent::Engine(Command::StudyMode(act)));
        }
        resp.should_close()
    }

    /// One card: the word, its sentence, and how to move through the deck.
    fn card_ui(
        &mut self,
        ui: &mut egui::Ui,
        cards: &[mareader_study::Card],
        t: &Tokens,
        action: &mut Option<Action>,
    ) {
        let last = cards.len().saturating_sub(1);
        let idx = self.card.min(last);
        let card = &cards[idx];
        ui.horizontal(|ui| {
            ui.label(RichText::new(card.lemma.clone()).size(28.0).strong());
            ui.label(RichText::new(card.pos.clone()).color(t.muted).italics());
            if card.count > 1 {
                ui.label(
                    RichText::new(format!("appears {}×", card.count))
                        .color(t.muted)
                        .small(),
                );
            }
        });
        ui.add_space(6.0);
        ui.add_space(8.0);
        ui.label(RichText::new("From the book").color(t.muted).small());
        ui.label(RichText::new(card.sentence.clone()).italics());
        ui.add_space(10.0);
        if !self.revealed {
            if ui.button("Reveal the meaning").clicked() {
                self.revealed = true;
            }
        } else {
            ui.label(RichText::new("Meaning").color(t.muted).small());
            ui.label(format!("Lemma {} · band {}", card.lemma, card.band));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("I already know this").clicked() {
                let key = mareader_study::word_key(&card.lemma, &card.pos);
                *action = Some(Action::MarkKnown(key));
                self.revealed = false;
                if self.card > 0 && self.card >= last {
                    self.card -= 1;
                }
            }
            if ui.button("Next").clicked() {
                self.card = (idx + 1) % cards.len();
                self.revealed = false;
            }
            if ui.button("Prev").clicked() {
                self.card = idx.checked_sub(1).unwrap_or(last);
                self.revealed = false;
            }
        });
    }

    /// Read back or start reading, with the strict timer's lock.
    fn footer(
        &mut self,
        ui: &mut egui::Ui,
        left: u32,
        strict: bool,
        t: &Tokens,
        action: &mut Option<Action>,
    ) {
        ui.horizontal(|ui| {
            if ui.button("Read previous pages").clicked() {
                *action = Some(Action::ReadBack);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let locked = strict && left > 0;
                let label = if locked {
                    format!("Start reading · {left}s")
                } else {
                    "Start reading".to_owned()
                };
                let fill = if locked { t.muted } else { t.accent };
                let btn =
                    ui.add_enabled(!locked, widgets::primary_button(ui, None, &label, fill, t));
                if btn.clicked() {
                    *action = Some(Action::StartReading);
                    self.reset();
                }
            });
        });
    }
}
