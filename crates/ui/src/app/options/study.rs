use super::*;

pub(super) fn set(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        #[cfg(not(target_arch = "wasm32"))]
        "dictionaries" => Some(app.option_dictionaries(key, value)),
        #[cfg(not(target_arch = "wasm32"))]
        "cefr" => Some(app.option_cefr(key, value)),
        "dict_window" => Some(app.option_dict_window(key, value)),
        "dict_fold" => Some(app.option_dict_fold(key, value)),
        "dict_page" => Some(app.option_dict_page(key, value)),
        "explain" => Some(app.option_explain(key, value)),
        "provider" => Some(app.option_provider(key, value)),
        "meaning" => Some(app.option_meaning(key, value)),
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
        let inline = self.study.inline();
        self.dict.attach(PathBuf::from(value), Arc::new(|| {}))?;
        self.dict.set_inline(inline);

        Ok(())
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn option_cefr(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let inline = self.study.inline();
        self.vocab.attach(PathBuf::from(value), Arc::new(|| {}))?;
        self.vocab.set_inline(inline);
        self.settings.study.cefr.enabled = true;

        Ok(())
    }

    // The floating Dictionary, searching for any word given.
    fn option_dict_window(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.dict_window(crate::dict::Window::Toggle);
        if value != "show" {
            self.dict.search(value);
        };

        Ok(())
    }

    // The floating Dictionary folded to its pill.
    fn option_dict_fold(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.dict_window(crate::dict::Window::Minimize(value == "true"));
        Ok(())
    }

    // The Dictionary page, searching for any word given.
    fn option_dict_page(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // After a queued open, which would otherwise leave it.
        match self.queued.iter().any(|c| matches!(c, Command::Open(_))) {
            true => self.queue(Command::ShowDictionary),
            false => self.go(Route::Dictionary),
        }
        if value != "show" {
            self.dict.search(value);
        };

        Ok(())
    }

    // Who explains a word in a shot: a sample, script or nobody.
    fn option_explain(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            "assistant" => self.study.set_explainer(Arc::new(AssistantSample)),
            "canned" => {
                self.study.set_explainer(Arc::new(mareader_study::Canned));
                self.page_ai.set_canned(true);
            }
            "none" => self
                .study
                .set_explainer(Arc::new(mareader_study::Unavailable)),
            _ => return Err("explain: assistant, canned or none".into()),
        };
        Ok(())
    }

    // One saved model on it explains words, e.g. `openai`.
    fn option_provider(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let name = serde_json::Value::String(value.to_owned());
        let p = serde_json::from_value(name).map_err(|_| "provider: e.g. openai or ollama")?;
        let ai = &mut self.settings.ai;
        ai.presets.clear();
        let id = ai.add_preset(p, "");
        ai.tasks.explain.preset = Some(id);

        Ok(())
    }

    // The meaning's language by tag, e.g. `my`.
    fn option_meaning(&mut self, _key: &str, value: &str) -> Result<(), String> {
        mareader_settings::language_name(value).ok_or("meaning: a tag, e.g. my")?;
        self.settings.ai.tasks.explain.language = Some(value.to_owned());

        Ok(())
    }

    // The first such text on a page selected, as `page:words`.
    fn option_selection(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "selection: page:text, as 0:quick";
        let (page, text) = value.split_once(':').ok_or(hint)?;
        let page = page.parse().map_err(|_| hint)?;
        let text = text.replace('_', " ");
        self.staged.push_back(Staged {
            page,
            text,
            explain: false,
            look_up: false,
            pen: None,
            note: None,
            turned: false,
            bookmark: false,
            pick: false,
        });

        Ok(())
    }

    // Picks marks, as `pick=on pick=on pick=style`: Style… opens.
    fn option_pick(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.staged.push_back(Staged {
            page: 0,
            text: String::new(),
            explain: false,
            look_up: false,
            pen: None,
            note: None,
            turned: true,
            bookmark: false,
            pick: value == "style",
        });

        Ok(())
    }

    // Bookmarks a page, as `bookmark=2`.
    fn option_bookmark(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let page = value.parse().map_err(|_| "bookmark: a page index")?;
        self.staged.push_back(Staged {
            page,
            text: String::new(),
            explain: false,
            look_up: false,
            pen: None,
            note: None,
            turned: false,
            bookmark: true,
            pick: false,
        });

        Ok(())
    }

    // Opens the word list window on pages 0-based: `word_window=0-4`.
    fn option_word_window(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "word_window: first-last, 0-based";
        let (a, b) = value.split_once('-').unwrap_or((value, value));
        let a: usize = a.parse().map_err(|_| hint)?;
        let b: usize = b.parse().map_err(|_| hint)?;
        self.word_window_seed = Some((a..=b.max(a)).collect());

        Ok(())
    }

    // Explains the staged selection: its card opens.
    fn option_gloss(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        match self.staged.back_mut() {
            Some(staged) => staged.explain = true,
            None => return Err("gloss: after selection=page:text".into()),
        };
        Ok(())
    }

    // Looks the staged selection up: the dictionary card opens.
    fn option_lookup(&mut self, _key: &str, _value: &str) -> Result<(), String> {
        match self.staged.back_mut() {
            Some(staged) => staged.look_up = true,
            None => return Err("lookup: after selection=page:text".into()),
        };
        Ok(())
    }

    // Notes the staged selection: a draft, or saved with `noted`.
    fn option_note_noted(&mut self, key: &str, value: &str) -> Result<(), String> {
        let staged = self.staged.back_mut().ok_or("note: after selection")?;
        staged.note = Some((value.replace('_', " "), key == "noted"));

        Ok(())
    }

    // Highlights the staged selection with a pen.
    fn option_highlight(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let hint = "highlight: full, half or underline, after selection";
        let pen =
            serde_json::from_value(serde_json::Value::String(value.into())).map_err(|_| hint)?;
        let staged = self.staged.back_mut().ok_or(hint)?;
        staged.pen = Some(pen);

        Ok(())
    }
}
