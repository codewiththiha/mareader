use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::command::Command;
impl MareaderApp {
    /// Delivers a finished instant word list to the clipboard or save dialog.
    pub(super) fn deliver_word_list(
        &mut self,
        ctx: &egui::Context,
        made: &mareader_engine::word_list::Made,
        output: mareader_settings::WordOutput,
    ) {
        let title = self
            .session
            .documents()
            .title(self.active())
            .unwrap_or_default()
            .to_owned();
        let toast = crate::word_list::deliver(ctx, &self.ui.files, &title, made, output);
        if let Some(toast) = toast {
            self.ui.toasts.push(toast);
        }
    }

    /// Who says English words on cards, which picks the accent they show.
    pub(super) fn english_voice(&self) -> Option<mareader_engine::sounds::EnglishVoice> {
        let others = self.settings().dictionary.other_voices;
        let picked = || {
            self.session
                .read_aloud()
                .english_voice(&self.settings().read_aloud, others)
        };
        self.ui.english.clone().or_else(picked)
    }

    /// How the open cards' words sound, as the settings write them.
    pub(super) fn card_sounds(
        &mut self,
        now: f64,
    ) -> Vec<(String, Vec<mareader_engine::sounds::Line>)> {
        let words = [self.ui.study.card_word(), self.ui.dict.card_word()];
        let mut words: Vec<String> = words.into_iter().flatten().map(str::to_owned).collect();
        words.extend(self.ui.dict.sound_words());
        let english = self.english_voice();
        let voice = mareader_engine::sounds::spoken(english.as_ref().map(|v| v.tag.as_str()));
        let pronunciation = self.settings().study.pronunciation;
        let shown = (&pronunciation, voice);
        words
            .into_iter()
            .filter_map(|w| self.session.sound_lines(&w, shown, now).map(|l| (w, l)))
            .collect()
    }

    /// Asks the tagger for the open card's word's role.
    pub(super) fn ask_role(&mut self) {
        if let Some((word, context)) = self.ui.study.card_words() {
            self.session.ask_role(word, context);
        }
    }

    /// A click on a shaded word: explained like a selected one.
    pub(super) fn explain_shaded(
        &mut self,
        pane: PaneId,
        page: usize,
        chars: std::ops::Range<usize>,
        rect: Rect,
    ) {
        let Some(asked) = self.documents().asked_at(pane, page, chars) else {
            return;
        };
        self.session.execute(Command::ClearSelection(pane), 0.0);
        self.ui.dict.escape();
        let book = self.book_in(pane);
        let name = self
            .session
            .documents()
            .title(pane)
            .unwrap_or_default()
            .to_owned();
        let at = Explained {
            pane,
            book,
            name: &name,
            asked,
            rect,
        };
        self.ui
            .study
            .explain(&mut self.session, at, mareader_engine::library::wall_ms());
        self.ask_role();
        self.probe_dict_for_study();
    }

    /// Resting on a shaded word looks it up, when a pack can answer.
    pub(super) fn rest(
        &mut self,
        pane: PaneId,
        (word, context): (String, String),
        (page, chars): (usize, std::ops::Range<usize>),
        at: Rect,
    ) {
        if !self.settings().dictionary.hover
            || self.session.dict().ready().is_empty()
            || self.ui.study.card_open()
        {
            return;
        }
        let offset = self.documents().offset(pane);
        let asked = self.documents().asked_at(pane, page, chars);
        let to = self.settings().dictionary.default_lang.clone();
        if self.ui.dict.rest(
            &mut self.session,
            pane,
            (word.clone(), context.clone()),
            (at, offset),
            (to, asked),
        ) {
            self.session.ask_role(&word, &context);
        }
    }

    /// Silently checks offline dictionaries so an AI card knows if tabs apply.
    pub(super) fn probe_dict_for_study(&mut self) {
        if !self.session.dict().attached() || self.session.dict().ready().is_empty() {
            return;
        }
        let Some((pane, word, at)) = self.ui.study.card_probe_info() else {
            return;
        };
        if self.ui.dict.has_stash_for(pane, &word) {
            return;
        }
        let offset = self.documents().offset(pane);
        let to = self.settings().dictionary.default_lang.clone();
        let query = mareader_dict::Query::LookUp {
            word: word.clone(),
            to,
        };
        let ticket = self.session.dict_ask(query);
        self.ui.dict.stash_probe(pane, word, ticket, at, offset);
    }

    /// Switches the open word card in place between Dictionary and AI.
    pub(super) fn word_card_mode(&mut self, mode: crate::word_card::Mode) {
        match mode {
            crate::word_card::Mode::Ai => {
                let Some(snap) = self.ui.dict.stash_for_tab() else {
                    return;
                };
                let pane = snap.pane;
                let book = self.book_in(pane);
                let name = self
                    .session
                    .documents()
                    .title(pane)
                    .unwrap_or_default()
                    .to_owned();
                let offset = self.documents().offset(pane);
                let wall_ms = mareader_engine::library::wall_ms();
                self.ui
                    .study
                    .open_from_tab(&mut self.session, book, &name, snap, offset, wall_ms);
                self.ask_role();
            }
            crate::word_card::Mode::Dictionary => {
                let Some(snap) = self.ui.study.stash_for_tab() else {
                    return;
                };
                let offset = self.documents().offset(snap.pane);
                if self.ui.dict.restore_from_tab(&snap, offset) {
                    return;
                }
                let to = self.settings().dictionary.default_lang.clone();
                let query = mareader_dict::Query::LookUp {
                    word: snap.word.clone(),
                    to,
                };
                let ticket = self.session.dict_ask(query);
                self.ui.dict.open_tab_lookup(snap, ticket, offset);
            }
        }
    }
}

/// The study's own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        #[cfg(not(target_arch = "wasm32"))]
        "dictionaries" => Some(app.option_dictionaries(key, value)),
        #[cfg(not(target_arch = "wasm32"))]
        "ai_dir" => Some(app.option_ai_dir(key, value)),
        "dict_window" => Some(app.option_dict_window(key, value)),
        "dict_fold" => Some(app.option_dict_fold(key, value)),
        "dict_page" => Some(app.option_dict_page(key, value)),
        "explain" => Some(app.option_explain(key, value)),
        "selection" => Some(app.option_selection(key, value)),
        "pick" => Some(app.option_pick(key, value)),
        "bookmark" => Some(app.option_bookmark(key, value)),
        "word_window" => Some(app.option_word_window(key, value)),
        "gloss" => Some(app.option_gloss(key, value)),
        "lookup" => Some(app.option_lookup(key, value)),
        "note" | "noted" => Some(app.option_note_noted(key, value)),
        "highlight" => Some(app.option_highlight(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    #[cfg(not(target_arch = "wasm32"))]
    fn option_dictionaries(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.session.wire_dict(PathBuf::from(value))
    }

    /// The AI service's folder, as a shot names one to answer Explain.
    #[cfg(not(target_arch = "wasm32"))]
    fn option_ai_dir(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let root = PathBuf::from(value);
        self.ui.ai.attach(&egui::Context::default(), root, None);
        // A run stopped before quitting waits on its card.
        self.session.library_set_paused(self.ui.ai.paused());
        Ok(())
    }

    // The floating Dictionary, searching for any word given.
    fn option_dict_window(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.dict_window(mareader_engine::dict::Window::Toggle);
        if value != "show" {
            self.ui.dict.search(value);
        };
        Ok(())
    }

    // The floating Dictionary folded to its pill.
    fn option_dict_fold(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.dict_window(mareader_engine::dict::Window::Minimize(value == "true"));
        Ok(())
    }

    // The Dictionary page, searching for any word given.
    fn option_dict_page(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // After a queued open, which would otherwise leave it.
        match self
            .ui
            .queued
            .iter()
            .any(|c| matches!(c, Sent::Engine(Command::Open(_))))
        {
            true => self.queue(Sent::Ui(UiCommand::ShowDictionary)),
            false => self.go(Route::Dictionary),
        }
        if value != "show" {
            self.ui.dict.search(value);
        };
        Ok(())
    }

    // Who explains a word in a shot: a sample, script or nobody.
    fn option_explain(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            "assistant" => self.session.study_set_explainer(Arc::new(AssistantSample)),
            "canned" => {
                self.session
                    .study_set_explainer(Arc::new(mareader_study::Canned));
                self.session.page_ai_set_canned(true);
            }
            "none" => self
                .session
                .study_set_explainer(Arc::new(mareader_study::Unavailable)),
            // Canned answers after a wait: `slow:20000`.
            other => {
                let hint = "explain: assistant, canned, slow:ms or none";
                let ms = other.strip_prefix("slow:").and_then(|ms| ms.parse().ok());
                let slow = mareader_study::Slow(ms.ok_or(hint)?);
                self.session.study_set_explainer(Arc::new(slow));
            }
        };
        Ok(())
    }

    // The first such text on a page selected, as `page:words`.
    fn option_selection(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "selection: page:text, as 0:quick";
        let (page, text) = value.split_once(':').ok_or(hint)?;
        let page = page.parse().map_err(|_| hint)?;
        let text = text.replace('_', " ");
        self.ui.staged.push_back(Staged {
            page,
            text,
            ..Staged::default()
        });

        Ok(())
    }

    // Picks marks, as `pick=on pick=on pick=style`: Style… opens.
    fn option_pick(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.staged.push_back(Staged {
            turned: true,
            pick: value == "style",
            ..Staged::default()
        });

        Ok(())
    }

    // Bookmarks a page, as `bookmark=2`.
    fn option_bookmark(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let page = value.parse().map_err(|_| "bookmark: a page index")?;
        self.ui.staged.push_back(Staged {
            page,
            bookmark: true,
            ..Staged::default()
        });

        Ok(())
    }

    // Opens the word list window on pages 0-based: `word_window=0-4`.
    fn option_word_window(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "word_window: first-last, 0-based";
        let (a, b) = value.split_once('-').unwrap_or((value, value));
        let a: usize = a.parse().map_err(|_| hint)?;
        let b: usize = b.parse().map_err(|_| hint)?;
        self.ui.word_window_seed = Some((a..=b.max(a)).collect());

        Ok(())
    }

    // Explains the staged selection: its card opens.
    fn option_gloss(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        match self.ui.staged.back_mut() {
            Some(staged) => staged.explain = true,
            None => return Err("gloss: after selection=page:text".into()),
        };
        Ok(())
    }

    // Looks the staged selection up: the dictionary card opens.
    fn option_lookup(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        match self.ui.staged.back_mut() {
            Some(staged) => staged.look_up = true,
            None => return Err("lookup: after selection=page:text".into()),
        };
        Ok(())
    }

    // Notes the staged selection: a draft, or saved with `noted`.
    fn option_note_noted(&mut self, key: &str, value: &str) -> Result<(), String> {
        let staged = self.ui.staged.back_mut().ok_or("note: after selection")?;
        staged.note = Some((value.replace('_', " "), key == "noted"));

        Ok(())
    }

    // Highlights the staged selection with a pen.
    fn option_highlight(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "highlight: full, half or underline, after selection";
        let pen =
            serde_json::from_value(serde_json::Value::String(value.into())).map_err(|_| hint)?;
        let staged = self.ui.staged.back_mut().ok_or(hint)?;
        staged.pen = Some(pen);

        Ok(())
    }
}
