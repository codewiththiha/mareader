//! Study mode inside `Session`: the switch, the gate and the lock.

use mareader_study::Mode;
use mareader_workspace::PaneId;

use crate::event::Event;
use crate::session::Session;
use crate::study_mode::{self, Action, State};
use crate::toast::Toast;

impl Session {
    /// Engine-owned study-mode state, read-only.
    pub fn study_mode(&self) -> &State {
        &self.study_mode
    }

    /// Runs a study-mode ask: set the mode, work the gate, memorize.
    pub fn study_mode_act(&mut self, action: Action, now_ms: u64) {
        match action {
            Action::Set(mode) => self.study_mode_set(mode),
            Action::MarkKnown(key) => {
                if self.study_mode.mark_known(key.clone())
                    && self.study_mode.mode() == Mode::Proactive
                {
                    self.emit(Event::Toast(Toast::info(format!(
                        "“{}” memorized in every book.",
                        key.split('|').next().unwrap_or(&key)
                    ))));
                }
            }
            Action::StartReading => self.study_mode_start(),
            Action::ReadBack => {
                self.study_mode.close_gate();
                self.emit(Event::Toast(Toast::info(
                    "Scroll back to re-read. The words open again at the end.",
                )));
            }
            Action::CloseGate => self.study_mode.close_gate(),
            Action::RecallDone {
                key,
                memorized,
                missed,
            } => {
                self.study_mode.note_recall(key.clone(), memorized, missed);
                if memorized {
                    self.study_mode.mark_known(key);
                }
            }
            Action::UnlockBatch => {
                if let Some(book) = self.active_book_key() {
                    let _ = self.study_mode.unlock_gate(book);
                }
            }
        }
        let _ = now_ms;
        self.persist_study_mode();
    }

    fn study_mode_set(&mut self, mode: Mode) {
        self.study_mode.set_mode(mode);
        if mode != Mode::Off {
            self.study_mode_replan();
            if mode == Mode::Proactive {
                self.emit(Event::Toast(Toast::info(
                    "Proactive study mode on. Words come first, then the pages.",
                )));
            } else {
                self.emit(Event::Toast(Toast::info(
                    "Reactive study mode on. Click a shaded word to recall it.",
                )));
            }
        }
        self.persist_study_mode();
    }

    /// Rebuilds batches from the active pane's page sizes.
    pub fn study_mode_replan(&mut self) {
        let Some(sizes) = self.active_page_sizes() else {
            return;
        };
        let dials = self.settings.study.mode.dials.clone();
        let cfg = study_mode::cfg_of(dials.unit, dials.pages, dials.chars);
        let _ = self.study_mode.plan(sizes, &cfg);
    }

    fn study_mode_start(&mut self) {
        let strict = self.settings.study.mode.dials.strict;
        if !self.study_mode.may_start(strict) {
            return;
        }
        let Some(book) = self.active_book_key() else {
            return;
        };
        let Some(gate) = self.study_mode.unlock_gate(book) else {
            return;
        };
        self.emit(Event::Toast(Toast::info(format!(
            "Unlocked {}. Read on.",
            gate.range()
        ))));
        self.persist_study_mode();
    }

    /// Content key for progress: the book, else the session document.
    fn active_book_key(&self) -> Option<u128> {
        Some(match self.book_in(self.active()) {
            Some(b) => b.0,
            None => {
                let name = self.documents().title(self.active()).unwrap_or_default();
                crate::study::loose::loose_id(name).0
            }
        })
    }

    /// Characters per page of the active pane, for the batch planner.
    fn active_page_sizes(&self) -> Option<Vec<usize>> {
        let pane = self.active();
        let count = self.documents().page_count(pane)?;
        let mut sizes = Vec::with_capacity(count);
        for page in 0..count {
            let n = self
                .documents()
                .layer(pane, page)
                .map_or(0, |l| l.text.chars().len());
            sizes.push(n);
        }
        Some(sizes)
    }

    /// Hook for save folding; unlocks live in study_mode state.
    fn persist_study_mode(&mut self) {}

    /// Steps the gate timer each second while a deck is open.
    pub fn study_mode_tick(&mut self) -> bool {
        self.study_mode.tick(crate::library::wall_ms())
    }

    /// Whether a jump to `page` in the active book is past the lock.
    pub fn study_mode_blocks(&self, page: usize) -> bool {
        let Some(book) = self.active_book_key() else {
            return false;
        };
        self.study_mode.blocks(book, page)
    }

    /// Opens the gate for the lock at the active pane's edge.
    pub fn study_mode_open_gate(&mut self, pane: PaneId) {
        if self.study_mode.gate().is_some_and(|g| g.pane == pane) {
            return;
        }
        let Some(book) = self.active_book_key() else {
            return;
        };
        let dials = self.settings.study.mode.dials.clone();
        let Some(batch) = self.study_mode.gate_batch(book).cloned() else {
            return;
        };
        let candidates = self.study_mode_candidates(book, &batch);
        let known = self.study_mode.memorized().clone();
        let deck = study_mode::deck_for(candidates, &known, dials.max_cards);
        if deck.cards.is_empty() {
            // Nothing new: unlock without a deck so reading continues.
            let _ = self.study_mode.unlock_gate(book);
            self.emit(Event::Toast(Toast::info(format!(
                "No new words in {}. Read on.",
                mareader_study::range_label(&batch)
            ))));
            return;
        }
        let _ = self.study_mode.open_gate(
            pane,
            book,
            deck,
            dials.sec_per_card,
            crate::library::wall_ms(),
        );
    }

    /// Hard words of the gate batch from page text and the level cache.
    fn study_mode_candidates(
        &mut self,
        _book: u128,
        batch: &mareader_study::Batch,
    ) -> Vec<mareader_study::Candidate> {
        let pane = self.active();
        let level = self.settings.study.cefr.level.band();
        let mut texts = Vec::new();
        for page in batch.from..=batch.to {
            if let Some(layer) = self.documents().layer(pane, page) {
                let (chars, _) = crate::study::words::read(&layer);
                let text: String = chars.into_iter().collect();
                texts.push((page, text));
            }
        }
        // Bands come from the vocab cache when attached; empty otherwise.
        let bands = self.vocab_bands_for(&texts);
        let known = self.study_mode.memorized().clone();
        study_mode::candidates_in(&texts, &bands, level, &known)
    }

    /// Asks vocab for each distinct key's band; offline builds answer empty.
    fn vocab_bands_for(
        &mut self,
        texts: &[(usize, String)],
    ) -> std::collections::HashMap<String, u8> {
        let mut keys: Vec<String> = Vec::new();
        for (_, text) in texts {
            for key in crate::word_marks::keys(&[crate::word_marks::Page {
                page: 0,
                chapter: None,
                text: text.clone(),
            }]) {
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
        }
        let bands = self.vocab.bands(&keys);
        keys.into_iter()
            .zip(bands)
            .filter_map(|(k, b)| b.map(|band| (k, band)))
            .collect()
    }
}

/// Guard used by save folding; unit tests pin the module links.
#[allow(dead_code)]
fn _linked(_: &Session) {}
