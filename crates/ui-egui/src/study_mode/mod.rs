//! Study mode on the page: the proactive deck and the reactive recall.
//!
//! The deck gates a batch of pages; recall asks what a shaded word means.
//! Both record through the engine, so progress stays with the book.

use egui::{Context, RichText};
use mareader_engine::command::Command;
use mareader_engine::session::Session;
use mareader_engine::study::candidates::candidates;
use mareader_engine::study::state::Key;
use mareader_engine::study::{Action, WordRecord};
use mareader_library::{LearnedBy, Spot, WordStatus};
use mareader_settings::StudyBatchUnit;
use mareader_study::{BatchCfg, BatchUnit, StudyMode, Verdict};
use mareader_workspace::PaneId;

use crate::ui_command::{Commands, Sent};

#[derive(Clone, Debug)]
struct DeckCard {
    lemma: String,
    context: String,
    page: usize,
    meaning: String,
    key: Option<Key>,
}

#[derive(Clone, Debug)]
struct Deck {
    pane: PaneId,
    cards: Vec<DeckCard>,
    idx: usize,
    revealed: bool,
    decisions: Vec<Option<bool>>,
    born: f64,
    batch_from: usize,
    batch_to: usize,
}

#[derive(Clone, Debug)]
struct Recall {
    pane: PaneId,
    lemma: String,
    context: String,
    page: usize,
    answer: String,
    attempts: u8,
    verdict: Option<Verdict>,
    feedback: String,
    hint: bool,
    key: Option<Key>,
    done: bool,
}

/// The deck and the recall popover, while the window lives.
#[derive(Default)]
pub struct StudyModeUi {
    deck: Option<Deck>,
    recall: Option<Recall>,
    dismissed: Option<(PaneId, usize)>,
}

fn spot(page: usize) -> Spot {
    Spot::Page {
        page,
        rect: [0.0; 4],
    }
}

fn record(pane: PaneId, word: WordRecord) -> Sent {
    Sent::Engine(Command::Study(Action::RecordWord(pane, word)))
}

/// How a word was learned, and how the attempt went.
struct Learn {
    by: LearnedBy,
    status: WordStatus,
    attempts: u8,
    first_try: bool,
}

fn word(lemma: &str, meaning: &str, context: &str, page: usize, learn: Learn) -> WordRecord {
    WordRecord {
        lemma: lemma.to_owned(),
        class: "other".to_owned(),
        band: 0,
        meaning: meaning.to_owned(),
        context: context.to_owned(),
        spot: spot(page),
        by: learn.by,
        status: learn.status,
        attempts: learn.attempts,
        first_try: learn.first_try,
    }
}

impl StudyModeUi {
    /// Opens recall on a shaded word in `pane`.
    pub fn open_recall(&mut self, pane: PaneId, word: &str, context: &str, page: usize) {
        self.recall = Some(Recall {
            pane,
            lemma: word.to_lowercase(),
            context: context.to_owned(),
            page,
            answer: String::new(),
            attempts: 0,
            verdict: None,
            feedback: String::new(),
            hint: false,
            key: None,
            done: false,
        });
    }

    pub fn recall_open(&self) -> bool {
        self.recall.is_some()
    }

    fn open_deck(&mut self, session: &mut Session, pane: PaneId, now: f64) {
        let cfg = session.settings().study.mode.clone();
        let current = session.documents().current(pane).unwrap_or(0);
        let count = session.documents().page_count(pane).unwrap_or(0);
        if count == 0 {
            return;
        }
        let mut sizes = Vec::with_capacity(count);
        for page in 0..count {
            let len = session
                .documents()
                .layer(pane, page)
                .map_or(0, |l| l.text.len());
            sizes.push(len);
        }
        let unit = match cfg.unit {
            StudyBatchUnit::Pages => BatchUnit::Pages,
            StudyBatchUnit::Chars => BatchUnit::Chars,
        };
        let batches = mareader_study::build_batches(
            &sizes,
            BatchCfg {
                unit,
                pages: cfg.pages as usize,
                chars: cfg.chars as usize,
            },
        );
        let Some(batch) = batches
            .iter()
            .find(|b| current >= b.from && current <= b.to)
            .or_else(|| batches.iter().find(|b| b.from >= current))
        else {
            return;
        };
        let (from, to) = (batch.from, batch.to);
        if self.dismissed == Some((pane, from)) {
            return;
        }
        let mut cards: Vec<DeckCard> = candidates(session, pane, from, to)
            .into_iter()
            .take(cfg.max_cards as usize)
            .map(|(lemma, context, page)| DeckCard {
                lemma,
                context,
                page,
                meaning: String::new(),
                key: None,
            })
            .collect();
        if cards.is_empty() {
            self.dismissed = Some((pane, from));
            return;
        }
        for card in &mut cards {
            card.key = Some(session.study_explain_loose(&card.lemma, &card.context));
        }
        let len = cards.len();
        self.deck = Some(Deck {
            pane,
            cards,
            idx: 0,
            revealed: false,
            decisions: vec![None; len],
            born: now,
            batch_from: from,
            batch_to: to,
        });
    }

    /// Draws the deck and the recall popover, and opens the deck at a gate.
    pub fn show(&mut self, ctx: &Context, session: &mut Session, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        let pane = session.active();
        if self.deck.is_none()
            && self.recall.is_none()
            && session.study_mode(pane) == StudyMode::Proactive
        {
            self.open_deck(session, pane, now);
        }
        self.show_deck(ctx, session, out, now);
        self.show_recall(ctx, session, out);
    }

    fn show_deck(&mut self, ctx: &Context, session: &mut Session, out: &mut Commands, now: f64) {
        let Some(mut deck) = self.deck.take() else {
            return;
        };
        let cfg = session.settings().study.mode.clone();
        let total = deck.cards.len();
        let idx = deck.idx.min(total.saturating_sub(1));
        let remaining = deck
            .decisions
            .iter()
            .filter(|d| d.is_none_or(|known| !known))
            .count();
        let floor = (cfg.sec_per_card as usize).saturating_mul(remaining) as f64;
        let waited = now - deck.born;
        let locked = cfg.strict && waited < floor;
        let mut close = false;
        let mut finish = false;
        if let Some(card) = deck.cards.get(idx).cloned() {
            let answer = card
                .key
                .and_then(|k| session.study().answer(k))
                .map(|s| s.info.clone());
            panel(ctx, "study-deck", "Study the next words", |ui| {
                deck_header(ui, &deck, idx, total, locked, waited, floor);
                ui.add_space(8.0);
                ui.label(RichText::new(&card.lemma).size(28.0).strong());
                ui.label(&card.context);
                ui.add_space(8.0);
                if deck.revealed {
                    deck_answer(ui, &card, &answer);
                    ui.horizontal(|ui| {
                        if ui.button("I know this").clicked() {
                            deck.decisions[idx] = Some(true);
                            deck.idx = deck.idx.saturating_add(1);
                            deck.revealed = false;
                        }
                        if ui.button("Still learning").clicked() {
                            deck.decisions[idx] = Some(false);
                            deck.idx = deck.idx.saturating_add(1);
                            deck.revealed = false;
                        }
                    });
                } else if ui.button("Show meaning").clicked() {
                    deck.revealed = true;
                }
                if deck.idx >= total {
                    finish = true;
                }
                ui.add_space(6.0);
                if ui.button("Turn study mode off").clicked() {
                    out.push(Sent::Engine(Command::Study(Action::SetMode(
                        deck.pane,
                        StudyMode::Off,
                    ))));
                    close = true;
                }
            });
        }
        if finish {
            self.finish_deck(&deck, out);
        } else if !close {
            self.deck = Some(deck);
        } else {
            self.dismissed = Some((deck.pane, deck.batch_from));
        }
    }

    fn finish_deck(&mut self, deck: &Deck, out: &mut Commands) {
        for (card, decision) in deck.cards.iter().zip(deck.decisions.iter()) {
            let status = match decision {
                Some(true) => WordStatus::Memorized,
                _ => WordStatus::Learning,
            };
            out.push(record(
                deck.pane,
                word(
                    &card.lemma,
                    &card.meaning,
                    &card.context,
                    card.page,
                    Learn {
                        by: LearnedBy::Card,
                        status,
                        attempts: 0,
                        first_try: false,
                    },
                ),
            ));
        }
        out.push(Sent::Engine(Command::Study(Action::Unlock(
            deck.pane,
            spot(deck.batch_to),
        ))));
        self.dismissed = None;
    }

    fn show_recall(&mut self, ctx: &Context, session: &mut Session, out: &mut Commands) {
        let Some(mut recall) = self.recall.take() else {
            return;
        };
        if recall.key.is_none() {
            recall.key = Some(session.study_explain_loose(&recall.lemma, &recall.context));
        }
        let answer = recall
            .key
            .and_then(|k| session.study().answer(k))
            .map(|s| s.info.clone());
        let ask_memorize = session.settings().study.mode.ask_memorize;
        let mut close = false;
        let mut keep = true;
        panel(ctx, "study-recall", "What does this mean?", |ui| {
            ui.label(RichText::new(&recall.lemma).size(24.0).strong());
            ui.label(&recall.context);
            if recall.hint {
                match &answer {
                    Some(info) => {
                        let hint: String = info.meaning.chars().take(1).collect();
                        ui.label(format!("Hint: it starts with “{hint}…”"));
                    }
                    None => {
                        ui.label("Hint: think of the sentence it sits in.");
                    }
                };
            }
            ui.text_edit_multiline(&mut recall.answer);
            if let Some(v) = recall.verdict {
                ui.label(RichText::new(v.label()).strong());
                ui.label(&recall.feedback);
            }
            ui.horizontal(|ui| {
                if !recall.done && ui.button("Check").clicked() {
                    check_recall(&mut recall, &answer, ask_memorize, out);
                }
                if !recall.done && ui.button("I don't know").clicked() {
                    recall.done = true;
                    recall.verdict = Some(Verdict::Wrong);
                    recall.feedback = answer
                        .as_ref()
                        .map_or_else(|| "Missed.".to_owned(), |i| i.meaning.clone());
                    out.push(record(
                        recall.pane,
                        word(
                            &recall.lemma,
                            answer.as_ref().map_or("", |i| i.meaning.as_str()),
                            &recall.context,
                            recall.page,
                            Learn {
                                by: LearnedBy::Recall,
                                status: WordStatus::Missed,
                                attempts: recall.attempts,
                                first_try: false,
                            },
                        ),
                    ));
                }
                if ui.button("Close").clicked() {
                    close = true;
                    keep = false;
                }
            });
        });
        if keep && !close {
            self.recall = Some(recall);
        }
    }
}

/// A revealed card's meaning, synonyms and usages.
fn deck_answer(ui: &mut egui::Ui, card: &DeckCard, answer: &Option<mareader_study::WordInfo>) {
    match answer {
        Some(info) => {
            ui.label(RichText::new(&info.meaning).strong());
            if !info.synonyms.is_empty() {
                ui.label(format!("Also: {}", info.synonyms.join(", ")));
            }
            for use_ in &info.usages {
                ui.label(use_);
            }
        }
        None if !card.meaning.is_empty() => {
            ui.label(&card.meaning);
        }
        None => {
            ui.label("Thinking of its meaning…");
        }
    }
}

/// Judges one recall attempt and records the word when it ends.
fn check_recall(
    recall: &mut Recall,
    answer: &Option<mareader_study::WordInfo>,
    ask_memorize: bool,
    out: &mut Commands,
) {
    recall.attempts = recall.attempts.saturating_add(1);
    let said = recall.answer.to_lowercase();
    let verdict = match &answer {
        Some(info)
            if !said.is_empty()
                && info
                    .meaning
                    .to_lowercase()
                    .split_whitespace()
                    .any(|w| w.len() > 3 && said.contains(w)) =>
        {
            Verdict::Correct
        }
        Some(_) if !said.is_empty() => Verdict::Close,
        _ => Verdict::Wrong,
    };
    recall.verdict = Some(verdict);
    recall.feedback = match verdict {
        Verdict::Correct => "That's it.".to_owned(),
        Verdict::Close => "Close — say a little more.".to_owned(),
        Verdict::Wrong => "Not quite — try again.".to_owned(),
    };
    if verdict != Verdict::Correct && mareader_study::hint_due(recall.attempts, verdict) {
        recall.hint = true;
    }
    if verdict == Verdict::Correct || recall.attempts >= mareader_study::MAX_ATTEMPTS {
        recall.done = true;
        let status = match verdict {
            Verdict::Correct if ask_memorize => WordStatus::Memorized,
            Verdict::Correct => WordStatus::Learning,
            _ => WordStatus::Missed,
        };
        out.push(record(
            recall.pane,
            word(
                &recall.lemma,
                answer.as_ref().map_or("", |i| i.meaning.as_str()),
                &recall.context,
                recall.page,
                Learn {
                    by: LearnedBy::Recall,
                    status,
                    attempts: recall.attempts,
                    first_try: recall.attempts == 1 && verdict == Verdict::Correct,
                },
            ),
        ));
    }
}

/// A deck's heading and its timer line.
fn deck_header(
    ui: &mut egui::Ui,
    deck: &Deck,
    idx: usize,
    total: usize,
    locked: bool,
    waited: f64,
    floor: f64,
) {
    ui.label(
        RichText::new(format!(
            "Batch · pages {}–{} · card {} of {}",
            deck.batch_from + 1,
            deck.batch_to + 1,
            idx + 1,
            total
        ))
        .strong(),
    );
    if locked {
        ui.label(format!(
            "Take your time — {}s left for these words.",
            (floor - waited).ceil() as u64
        ));
    }
}

/// A centred floating panel for the deck and recall, on the tool tier.
fn panel(ctx: &Context, id: &'static str, title: &'static str, add: impl FnOnce(&mut egui::Ui)) {
    crate::z_order::area(ctx, crate::z_order::Tier::FloatingTool, egui::Id::new(id))
        .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::new()
                .fill(ui.style().visuals.window_fill)
                .stroke(egui::Stroke::new(
                    1.0,
                    ui.style().visuals.window_stroke.color,
                ))
                .corner_radius(12.0)
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.set_max_width(430.0);
                    ui.label(RichText::new(title).strong());
                    ui.add_space(6.0);
                    add(ui);
                });
        });
}
