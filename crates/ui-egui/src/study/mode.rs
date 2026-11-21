//! Study mode overlays: the proactive gate, the deck, and the reactive
//! recall popover; see `docs/study-mode.md`.
//!
//! Drawn over the reading stage when study mode is on. Nothing here names a
//! provider: the card maker and the judge are injected from the app (AI or
//! dictionary pack), with canned stand-ins so every state can run offline.

use std::time::Instant;

use crate::z_order::{self, Tier};
use egui::{Button, Color32, Frame, Id, Margin, Popup, Pos2, Rect, RichText, ScrollArea, TextEdit};
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::study::Action as StudyAction;
use mareader_library::{BookId, Spot};
use mareader_study::{Card, JudgedEntry, StudyMode, StudyModeSettings, Verdict};
use mareader_workspace::PaneId;

use crate::theme::Tokens;
use crate::ui_command::{Commands, Sent};

/// The proactive gate and the reactive recall popover, over one pane's stage.
#[derive(Default)]
pub struct StudyModeUi {
    /// The recall popover over a shaded word, or none.
    recall: Option<Recall>,
    /// The deck the proactive gate is showing, or none.
    deck: Option<Deck>,
    /// When the proactive gate first opened this frame, for the floor timer.
    gate_opened_at: Option<Instant>,
}

/// The reader's attempt at one shaded word, mid-popover.
#[allow(dead_code)]
struct Recall {
    pane: PaneId,
    /// The normalised word, also the card's key.
    word: String,
    /// The sentence it appears in.
    sentence: String,
    /// Where the word sits on screen, so the popover can sit beside it.
    at: Rect,
    /// What the reader has typed.
    answer: String,
    /// The judge's verdict once submitted, or none.
    verdict: Option<(Verdict, String)>,
    /// How many times the reader has submitted.
    attempts: u8,
}

/// A batch's deck, ready to study: its cards and the floor timer.
#[allow(dead_code)]
struct Deck {
    book: BookId,
    spot: Spot,
    cards: Vec<Card>,
    /// Seconds left on the floor timer; none until studying starts.
    left: Option<f64>,
}

/// Data passed from the gate controller to the gate UI render.
struct GateView {
    cards_empty: bool,
    cards_len: usize,
    cards: Vec<Card>,
    remaining: u32,
    time_up: bool,
}

impl StudyModeUi {
    /// Opens the recall popover over a shaded word the reader clicked.
    pub fn open_recall(&mut self, pane: PaneId, word: String, sentence: String, at: Rect) {
        self.deck = None;
        self.recall = Some(Recall {
            pane,
            word: word.trim().to_owned(),
            sentence,
            at,
            answer: String::new(),
            verdict: None,
            attempts: 0,
        });
    }

    /// Draws the recall popover and, while proactive, the gate over `stage`.
    /// Returns whether something is shown over the page.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        stage: Rect,
        out: &mut Commands,
    ) -> bool {
        let mode = session.study_mode().mode;
        if mode == StudyMode::Off {
            self.recall = None;
            self.deck = None;
            return false;
        }

        let mut shown = false;
        if let Some(recall) = self.recall.take() {
            shown = true;
            self.recall = self.recall_popup(ctx, session, stage, recall, out);
        } else if mode == StudyMode::Proactive {
            shown = true;
            self.gate_opened_at.get_or_insert(Instant::now());
            self.gate(ctx, session, stage, out);
        } else {
            self.gate_opened_at = None;
        }
        shown
    }

    /// The recall popover, fixed beside the word on screen.
    fn recall_popup(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        _stage: Rect,
        mut recall: Recall,
        out: &mut Commands,
    ) -> Option<Recall> {
        let id = Id::new("study-recall");
        let t = Tokens::get(ctx);
        let mut close = false;
        let at = recall.at;
        Popup::new(
            id,
            ctx.clone(),
            Pos2::new(at.left(), at.top()),
            z_order::layer(Tier::Popover, id),
        )
        .show(|ui| {
            let width = 320.0;
            ui.set_min_width(width);
            Frame::group(ui.style())
                .inner_margin(Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.heading(RichText::new("Recall").strong());
                    ui.label(RichText::new(&recall.word).strong().color(t.accent));
                    ui.label(RichText::new("As in: ").small().color(t.muted));
                    ui.label(RichText::new(&recall.sentence).small().italics());
                    ui.add_space(8.0);
                    ui.label("What does it mean here?");
                    let answer = TextEdit::singleline(&mut recall.answer)
                        .hint_text("A synonym, a short explanation, or a translation");
                    let r = ui.add(answer.desired_width(f32::INFINITY));
                    if (r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
                        && !recall.answer.is_empty()
                    {
                        self.submit(&mut recall, session, out);
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if ui.add(Button::new("Submit")).clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Enter))
                        {
                            self.submit(&mut recall, session, out);
                        }
                        ui.add_space(8.0);
                        if ui.add(Button::new("Give up")).clicked() {
                            self.give_up(&mut recall);
                            close = true;
                        }
                        ui.add_space(8.0);
                        if ui.add(Button::new("Close")).clicked() {
                            close = true;
                        }
                    });
                    if let Some((verdict, feedback)) = &recall.verdict {
                        ui.add_space(8.0);
                        let color = match verdict {
                            Verdict::Correct => t.accent,
                            Verdict::Close => t.track,
                            Verdict::Wrong => Color32::from_rgb(0xef, 0x5a, 0x5a),
                        };
                        ui.label(RichText::new(format!("{verdict} — {feedback}")).color(color));
                    }
                    if recall.verdict.is_some() && recall.attempts >= 3 {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Three tries are up; the word stays in rotation.")
                                .small()
                                .color(t.muted),
                        );
                        close = true;
                    }
                });
        });
        if close { None } else { Some(recall) }
    }

    /// Judges the reader's answer and writes the result.
    fn submit(&mut self, recall: &mut Recall, session: &Session, out: &mut Commands) {
        let Some(judge) = session.study_mode().judge.as_ref() else {
            recall.verdict = Some((Verdict::Wrong, "No judge is set up yet.".into()));
            recall.attempts += 1;
            return;
        };
        let entry = JudgedEntry {
            lemma: recall.word.clone(),
            synonyms: Vec::new(),
            meaning: String::new(),
        };
        let judged = match judge.judge(&recall.answer, &entry) {
            Ok(judged) => judged,
            Err(_) => {
                recall.verdict = Some((Verdict::Wrong, "The judge could not answer.".into()));
                recall.attempts += 1;
                return;
            }
        };
        recall.attempts += 1;
        let correct = judged.verdict.is_correct();
        recall.verdict = Some((judged.verdict, judged.feedback));
        if correct {
            out.push(Sent::Engine(Command::Study(StudyAction::MarkKnown {
                key: recall.word.clone(),
            })));
        }
    }

    /// Reveals the answer and closes.
    fn give_up(&mut self, recall: &mut Recall) {
        recall.verdict = Some((
            Verdict::Wrong,
            "Revealed: the word stays in rotation.".into(),
        ));
        recall.attempts = 3;
    }

    /// The proactive gate: learn the batch's words before reading on.
    fn gate(&mut self, ctx: &egui::Context, session: &Session, stage: Rect, _out: &mut Commands) {
        let settings = &session.study_mode().settings;
        let Some(book) = session.book_in(session.active()) else {
            return;
        };
        self.deck.get_or_insert_with(|| Deck {
            book,
            spot: Spot::Page {
                page: 0,
                rect: [0.0, 0.0, 0.0, 0.0],
            },
            cards: Vec::new(),
            left: None,
        });
        let Some(deck) = self.deck.as_ref() else {
            return;
        };
        let (cards_empty, cards_len, cards) =
            (deck.cards.is_empty(), deck.cards.len(), deck.cards.clone());
        let (remaining, time_up) = Self::gate_timer(settings, cards_len, self.gate_opened_at);
        let view = GateView {
            cards_empty,
            cards_len,
            cards,
            remaining,
            time_up,
        };

        self.gate_ui(ctx, stage, settings, &view);
    }

    /// Remaining timer seconds and whether time is up, when strict.
    fn gate_timer(
        settings: &StudyModeSettings,
        cards_len: usize,
        opened_at: Option<Instant>,
    ) -> (u32, bool) {
        let total_secs = settings.sec_per_card as u32 * cards_len as u32;
        let elapsed = opened_at.map(|t| t.elapsed().as_secs() as u32).unwrap_or(0);
        let remaining = total_secs.saturating_sub(elapsed);
        (remaining, remaining == 0)
    }

    /// Renders the gate UI over `stage`.
    fn gate_ui(
        &mut self,
        ctx: &egui::Context,
        stage: Rect,
        settings: &StudyModeSettings,
        view: &GateView,
    ) {
        let total_secs = settings.sec_per_card as u32 * view.cards_len as u32;
        z_order::area(ctx, Tier::FloatingTool, Id::new("study-gate"))
            .fixed_pos(stage.min)
            .show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    Frame::group(ui.style())
                        .inner_margin(Margin::symmetric(24, 18))
                        .show(ui, |ui| {
                            ui.set_max_width(520.0);
                            ui.heading(
                                RichText::new("Study this batch before reading on").strong(),
                            );
                            ui.add_space(8.0);
                            ui.label("Learn the words, then the batch unlocks.");
                            ui.add_space(12.0);
                            if view.cards_empty {
                                ui.label(
                                    "The deck is being prepared; keep reading while it arrives.",
                                );
                            } else {
                                ui.label(format!(
                                    "{} card{}",
                                    view.cards_len,
                                    if view.cards_len == 1 { "" } else { "s" }
                                ));
                                ui.add_space(6.0);
                                ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                                    for card in &view.cards {
                                        ui.separator();
                                        ui.label(RichText::new(&card.key).strong());
                                        ui.label(RichText::new(&card.meaning).small());
                                    }
                                });
                            }
                            ui.add_space(12.0);
                            if settings.strict && total_secs > 0 {
                                let mins = view.remaining / 60;
                                let secs = view.remaining % 60;
                                ui.label(format!("Time remaining: {mins}:{secs:02}",));
                                if !view.time_up {
                                    ctx.request_repaint();
                                }
                            }
                            ui.add_space(12.0);
                            let ready = !settings.strict || view.time_up || view.cards_empty;
                            let label = if settings.strict && total_secs > 0 && !view.time_up {
                                format!("Unlock in {} s", view.remaining)
                            } else {
                                "I know them — unlock the batch".to_owned()
                            };
                            if ui.add_enabled(ready, Button::new(label)).clicked() {
                                self.deck = None;
                                self.gate_opened_at = None;
                            }
                        });
                });
            });
    }
}
