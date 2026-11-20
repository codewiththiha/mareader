//! Reactive mode's recall: a click on a shaded word asks for its meaning.

use egui::{Align, Context, Layout, RichText, TextEdit};
use mareader_engine::command::Command;
use mareader_engine::study_mode::Action;
use mareader_settings::StudyModeSettings;
use mareader_study::{Mode, RecallTarget, Verdict, judge, word_key};

use crate::modal::{self, Dialog};
use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent, UiCommand};
use mareader_engine::toast::Toast;

/// How long a check pretends to think; the real path waits on StudyJudge.
const CHECK_MS: u64 = 700;
/// Tries the popover allows before it only offers the answer.
const MAX_TRIES: u32 = 3;

/// One open recall: the word, the answer box, and where it stands.
pub struct RecallUi {
    /// The word as the page wrote it.
    word: String,
    /// The sentence around it, for feedback and records.
    context: String,
    /// CEFR level dial, shown beside the attempts.
    level: u8,
    /// Whether to ask before marking memorized.
    ask_memorize: bool,
    target: RecallTarget,
    phase: Phase,
    attempt: String,
    tries: u32,
    verdict: Verdict,
    feedback: String,
    /// Wall ms when Check was pressed, for the short delay.
    checking_since: Option<u64>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Input,
    Checking,
    Result,
    Ask,
    Revealed,
}

impl Default for RecallUi {
    fn default() -> Self {
        Self {
            word: String::new(),
            context: String::new(),
            level: 3,
            ask_memorize: true,
            target: RecallTarget {
                lemma: String::new(),
                meaning: String::new(),
                synonyms: Vec::new(),
                gloss: String::new(),
            },
            phase: Phase::Input,
            attempt: String::new(),
            tries: 0,
            verdict: Verdict::Wrong,
            feedback: String::new(),
            checking_since: None,
        }
    }
}

impl RecallUi {
    /// Opens the popover for one shaded word.
    pub fn open(&mut self, word: &str, context: &str, meaning: &str, gloss: &str) {
        self.word = word.to_owned();
        self.context = context.to_owned();
        self.target = RecallTarget {
            lemma: word.to_lowercase(),
            meaning: meaning.to_owned(),
            synonyms: Vec::new(),
            gloss: gloss.to_owned(),
        };
        self.phase = Phase::Input;
        self.attempt.clear();
        self.tries = 0;
        self.verdict = Verdict::Wrong;
        self.feedback.clear();
        self.checking_since = None;
    }

    pub fn is_open(&self) -> bool {
        !self.word.is_empty()
    }

    pub fn close(&mut self) {
        self.word.clear();
        self.phase = Phase::Input;
        self.checking_since = None;
    }

    /// Draws the popover; true when it finished or the user dismissed it.
    pub fn show(
        &mut self,
        ctx: &Context,
        settings: &StudyModeSettings,
        out: &mut Commands,
    ) -> bool {
        if !self.is_open() || settings.mode != Mode::Reactive {
            return false;
        }
        self.ask_memorize = settings.dials.ask_memorize;
        self.level = settings.dials.level;
        let t = Tokens::get(ctx);
        let mut done = false;
        let mut finish = None;
        let resp = Dialog::new("study-recall").show(ctx, |ui| {
            self.body(ui, ctx, &t, &mut done, &mut finish);
        });
        if self.phase == Phase::Checking
            && let Some(since) = self.checking_since
        {
            let now = ctx.input(|i| i.time * 1000.0) as u64;
            if now.saturating_sub(since) >= CHECK_MS {
                self.verdict_check();
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        }
        if resp.should_close() {
            done = true;
        }
        if let Some(status) = finish {
            self.finish(status, out);
            return true;
        }
        if done {
            self.close();
            return true;
        }
        false
    }

    /// The dialog's contents: header, then the phase's controls.
    fn body(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        t: &Tokens,
        done: &mut bool,
        finish: &mut Option<&'static str>,
    ) {
        ui.set_min_width(340.0);
        ui.set_max_width(420.0);
        modal::heading(ui, None, "Recall", None, t);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("Level {}", self.level))
                    .color(t.accent)
                    .small()
                    .strong(),
            );
            for i in 0..MAX_TRIES {
                let on = i < self.tries;
                let (rect, _) = ui.allocate_exact_size(egui::vec2(18.0, 4.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rect, 2.0, if on { t.accent } else { t.line });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("✕").clicked() {
                    *done = true;
                }
            });
        });
        ui.add_space(8.0);
        ui.label(format!("What does “{}” mean here?", self.word));
        ui.add_space(8.0);
        match self.phase {
            Phase::Checking => {
                ui.label(RichText::new("Checking…").color(t.muted).small());
            }
            Phase::Input => self.input_ui(ui, ctx, t),
            Phase::Result => self.result_ui(ui, t, finish),
            Phase::Ask => self.ask_ui(ui, t, finish),
            Phase::Revealed => self.revealed_ui(ui, t, finish),
        }
    }

    fn input_ui(&mut self, ui: &mut egui::Ui, ctx: &Context, t: &Tokens) {
        let mut enter = false;
        let edit = TextEdit::multiline(&mut self.attempt)
            .desired_rows(2)
            .hint_text("In your own words, in any language…");
        let r = ui.add(edit);
        if r.lost_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            enter = true;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.small_button("I don't know").clicked() {
                self.phase = Phase::Revealed;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let can = !self.attempt.trim().is_empty();
                let now = ctx.input(|i| i.time * 1000.0) as u64;
                if (ui.add_enabled(can, egui::Button::new("Check")).clicked() || (enter && can))
                    && can
                {
                    self.start_check(now);
                }
            });
        });
        let _ = t;
    }

    /// Sends the verdict to the engine and closes.
    fn finish(&mut self, status: &'static str, out: &mut Commands) {
        let key = word_key(&self.target.lemma, "any");
        let memorized = status == "memorized";
        let missed = status == "missed";
        out.push(Sent::Engine(Command::StudyMode(Action::RecallDone {
            key,
            memorized,
            missed,
        })));
        let msg = if memorized {
            format!("“{}” memorized. Saved to Space.", self.word)
        } else if missed {
            format!("“{}” saved as missed.", self.word)
        } else {
            format!("“{}” kept for practice.", self.word)
        };
        out.push(Sent::Ui(UiCommand::Notify(Toast::info(msg))));
        self.close();
    }

    fn start_check(&mut self, now_ms: u64) {
        self.phase = Phase::Checking;
        self.checking_since = Some(now_ms);
    }

    fn verdict_check(&mut self) {
        let judged = judge(&self.attempt, &self.target);
        self.tries += 1;
        self.verdict = judged.verdict;
        self.feedback = judged.feedback;
        self.checking_since = None;
        if judged.verdict == Verdict::Correct {
            if self.target.meaning.trim().is_empty() && self.target.gloss.trim().is_empty() {
                // No gloss yet: near miss until a pack fills the meaning.
                self.phase = Phase::Result;
            } else {
                self.phase = Phase::Ask;
            }
        } else {
            self.phase = Phase::Result;
        }
    }

    fn result_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, finish: &mut Option<&'static str>) {
        let tone = match self.verdict {
            Verdict::Correct => t.accent,
            Verdict::Close => t.danger,
            Verdict::Wrong => t.danger,
        };
        let head = match self.verdict {
            Verdict::Correct => "Correct",
            Verdict::Close => "Almost",
            Verdict::Wrong => "Not quite",
        };
        ui.label(RichText::new(head).strong().color(tone));
        ui.label(RichText::new(self.feedback.clone()).color(t.ink));
        ui.add_space(8.0);
        if self.tries == 1 && self.verdict != Verdict::Correct {
            ui.label(
                RichText::new("Hint: a synonym starts with the same letters as a common gloss.")
                    .color(t.muted)
                    .small(),
            );
        }
        ui.horizontal(|ui| {
            if ui.small_button("Show answer").clicked() {
                self.phase = Phase::Revealed;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.tries < MAX_TRIES
                    && ui
                        .add_enabled(true, egui::Button::new("Try again"))
                        .clicked()
                {
                    self.attempt.clear();
                    self.phase = Phase::Input;
                } else if self.tries >= MAX_TRIES && ui.button("Give up").clicked() {
                    *finish = Some("missed");
                }
            });
        });
    }

    fn ask_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, finish: &mut Option<&'static str>) {
        ui.label(
            RichText::new(if self.tries == 1 {
                "Correct, first try"
            } else {
                "Correct"
            })
            .strong()
            .color(t.accent),
        );
        ui.label(RichText::new(self.feedback.clone()).color(t.ink));
        ui.add_space(8.0);
        ui.label(format!("Add “{}” to memorized?", self.word));
        ui.label(
            RichText::new("It stops being shaded and won’t come back in flash cards, in any book.")
                .color(t.muted)
                .small(),
        );
        ui.horizontal(|ui| {
            if ui.small_button("Keep practising").clicked() {
                *finish = Some("learning");
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Memorized").clicked() {
                    *finish = Some("memorized");
                }
            });
        });
    }

    fn revealed_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, finish: &mut Option<&'static str>) {
        let shown = if self.target.meaning.trim().is_empty() {
            if self.target.gloss.trim().is_empty() {
                "No gloss yet — the card’s AI can explain it.".to_owned()
            } else {
                self.target.gloss.clone()
            }
        } else {
            self.target.meaning.clone()
        };
        ui.label(RichText::new(format!("“{}”", self.word)).strong());
        ui.label(shown);
        if !self.context.is_empty() {
            ui.label(
                RichText::new(self.context.clone())
                    .italics()
                    .small()
                    .color(t.muted),
            );
        }
        ui.add_space(8.0);
        if self.ask_memorize {
            ui.horizontal(|ui| {
                if ui.small_button("Keep practising").clicked() {
                    *finish = Some("learning");
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Memorized").clicked() {
                        *finish = Some("memorized");
                    }
                });
            });
        } else {
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Done").clicked() {
                        *finish = Some("learning");
                    }
                });
            });
        }
    }
}
