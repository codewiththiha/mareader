use super::*;
use mareader_engine::documents::OpenRequest;

impl MareaderApp {
    /// The voice booth opens with a voice tab and closes after it.
    pub(super) fn voices_shown(&mut self, ctx: &Context) {
        use mareader_engine::settings::Tab;
        let engine = match self.ui.settings_window.tab() {
            _ if !self.ui.settings_window.is_open() => None,
            Tab::ReadAloud => Some(self.settings().read_aloud.engine.clone()),
            Tab::Model(m) => Some(m.id.to_owned()),
            _ => None,
        };
        if let Some(wait) = self.session.read_aloud_voices_shown(engine.as_deref()) {
            ctx.request_repaint_after(wait);
        }
    }

    /// Opens the book read aloud beside what shows, to read on there.
    pub(super) fn reopen_reading(
        &mut self,
        ctx: &Context,
        origin: mareader_engine::read_aloud::state::Origin,
    ) {
        let name = &origin.name;
        // Checked first, so a lost file opens no empty pane.
        let why = match &origin.path {
            Some(p) if !p.exists() => Some(format!("“{name}” was moved or deleted")),
            None => Some(format!("“{name}” didn't come from a file")),
            Some(_) => None,
        };
        if let Some(why) = why {
            let note = format!("{why}, so it can't open again. Reading goes on.");
            self.ui.toasts.push(Toast::warning(note));
            return;
        }
        let shelved = origin
            .book
            .filter(|&b| self.session.library_model().book(b).is_some());
        if let Some(book) = shelved {
            self.go(Route::Reader);
            let place = Command::PlaceBook {
                book,
                pane: None,
                placement: Placement::Beside,
            };
            return self.apply(ctx, Sent::Engine(place));
        }
        let Some(path) = origin.path else {
            return;
        };
        let empty = self.session.documents().title(self.active()).is_none();
        let placement = if empty {
            Placement::Into
        } else {
            Placement::Beside
        };
        let request = OpenRequest {
            pane: Some(self.active()),
            placement,
            ..OpenRequest::path(path)
        };
        self.apply(ctx, Sent::Engine(Command::Open(request)));
    }

    /// The read-aloud player, on any page: a card when the words are away.
    pub(super) fn player(&mut self, ctx: &Context, route: Route, out: &mut Commands) {
        let reader = route == Route::Reader;
        let spot = self.session.read_aloud().spot();
        let shown = spot
            .clone()
            .filter(|_| reader)
            .and_then(|(pane, page, chars)| {
                let (_, rect) = self.ui.pane_rects.iter().find(|(p, _)| *p == pane)?;
                let room = rect.with_max_y(rect.bottom() - crate::page_bar::BAR_H);
                let docs = self.documents();
                let on = docs.visible(pane)?.contains(&page);
                match chars.and_then(|c| docs.hit_box(pane, page, c)) {
                    Some(hit) => Some(room.intersects(hit)),
                    None => Some(on),
                }
            });
        let origin = self.session.read_aloud().origin();
        let title = origin.map_or("Read aloud", |o| o.name.as_str()).to_owned();
        let book = origin.and_then(|o| o.book);
        let mut content = ctx.content_rect();
        content.min.y += crate::theme::TITLE_BAR_H;
        // Panes with looks of their own: it wears its book's, unlike the rest.
        let home = self.session.read_aloud().home();
        let shows = |pane: &PaneId| self.ui.pane_rects.iter().any(|(p, _)| p == pane);
        let tint = (home.filter(shows))
            .filter(|_| reader && self.routing().any())
            .map(|pane| theme::Tokens::new(&self.look(pane)));
        let lift = match route {
            Route::Reader => self.toast_lift(ctx, ctx.content_rect()),
            Route::Library | Route::Dictionary => 0.0,
        };
        let dict = (reader).then(|| self.ui.dict.window_cover()).flatten();
        let active = self.session.read_aloud().active();
        let toasts = self.ui.toasts.active_bed_for(
            ctx,
            ctx.content_rect(),
            lift,
            self.ui.read_aloud.island_anchor(active),
            dict,
        );
        let parking = self.settings().workspace.edge_parking;
        let stage = crate::read_aloud::Stage {
            reader,
            panes: &self.ui.pane_rects,
            content,
            shown,
            title: &title,
            book,
            tint,
            toasts,
            dict,
            parking,
        };
        let mut settings = self.settings().read_aloud.clone();
        let model = self.session.library_model();
        let aloud = self.session.read_aloud();
        let library = &mut self.ui.library;
        let mut cover = |ui: &egui::Ui, rect: egui::Rect| {
            book.is_some_and(|b| library.paint_cover(ui, rect, model, b))
        };
        let ui = &mut self.ui.read_aloud;
        let act = ui.bar(ctx, aloud, stage, &mut settings, out, &mut cover);
        if let Some(act) = act {
            let now = ctx.input(|i| i.time);
            self.session.execute(Command::ReadAloudAct(act), now);
        }
        let _ = self.set_field("read_aloud", settings);
    }
}

/// The voice's own keys: what reads this window's own state.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "english_voice" => Some(app.option_english_voice(key, value)),
        #[cfg(not(target_arch = "wasm32"))]
        "voice_models" => Some(app.option_voice_models(key, value)),
        "read_aloud" => Some(app.option_read_aloud(key, value)),
        "transcript" => Some(app.option_transcript(key, value)),
        "trigger" => Some(app.option_trigger(key, value)),
        "script" => Some(app.option_script(key, value)),
        "settings" => Some(app.option_settings(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    // The English voice, as if picked: `english_voice=Karen:en-AU`.
    fn option_english_voice(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (name, tag) = value.split_once(':').ok_or("english_voice=name:tag")?;
        let (name, tag) = (name.replace('_', " "), tag.to_owned());
        self.ui.english = Some(mareader_engine::sounds::EnglishVoice { name, tag });

        Ok(())
    }

    // The models on offer, kept in this folder.
    #[cfg(not(target_arch = "wasm32"))]
    fn option_voice_models(&mut self, _key: &str, value: &str) -> Result<(), String> {
        let (dir, models) = (PathBuf::from(value), mareader_speech::MODELS);
        self.session.wire_voices(dir, models)
    }

    fn option_read_aloud(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui.read_aloud_shot = value == "true";
        Ok(())
    }

    fn option_transcript(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `card` or `card:track`, comma separated.
        for one in value.split(',') {
            let (card, track) = one.split_once(':').unwrap_or((one, ""));
            let card = card.parse().map_err(|_| "transcript: card[:track],…")?;
            self.ui.transcript_shot.push((card, track.parse().ok()));
        }

        Ok(())
    }

    fn option_trigger(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // `target` shows it, `target:hide` hides it; comma separated.
        for one in value.split(',') {
            let (target, how) = one.split_once(':').unwrap_or((one, "show"));
            let target = target.parse().map_err(|_| "trigger: target[:hide],…")?;
            self.ui.trigger_shot.push((target, how != "hide"));
        }

        Ok(())
    }

    // Clicks the first element each selector matches; comma separated.
    fn option_script(&mut self, _key: &str, value: &str) -> Result<(), String> {
        self.ui
            .script_shot
            .extend(value.split(',').map(str::to_owned));
        Ok(())
    }

    fn option_settings(&mut self, _key: &str, value: &str) -> Result<(), String> {
        // A downloaded model's tab goes by the model's id.
        let model = self
            .session
            .read_aloud()
            .shelf
            .ready()
            .into_iter()
            .find(|m| m.id == value);
        let tab = Tab::parse(value).or(model.map(Tab::Model));
        let tab = tab.ok_or("settings: a tab name, e.g. theme")?;
        self.open_settings(Some(tab));

        Ok(())
    }
}
