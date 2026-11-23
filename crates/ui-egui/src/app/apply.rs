use super::navigation::found_in;
use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::event::{Event, PaneAction};
use mareader_engine::session::EdgeTab;

/// The Dictionary's tab parked on the window's edge.
const EDGE_TAB: &str = "dict-edge-tab-button";

impl MareaderApp {
    /// A command goes to the engine; chrome is carried out here.
    pub(super) fn apply(&mut self, ctx: &Context, command: Sent) {
        if !self.route().allows(&command) {
            return;
        }
        // Text edits must reach the pane focused when they were made.
        self.reconcile_texts();
        match command {
            Sent::Engine(intent) => {
                let now = ctx.input(|i| i.time);
                self.session.set_area(Self::area(ctx));
                self.session.set_edge_tab(EdgeTab {
                    shown: self.ui.dict.edge_tab_active(),
                    focused: ctx.memory(|m| m.has_focus(egui::Id::new(EDGE_TAB))),
                });
                self.session.execute(intent, now);
                self.drain(ctx);
            }
            Sent::Ui(ui) => self.chrome(ctx, ui),
        }
    }

    /// Shows what the session reported since it was last asked.
    pub(super) fn drain(&mut self, ctx: &Context) {
        for event in std::mem::take(&mut self.ui.deferred_events) {
            self.handle_event(ctx, event);
        }
        for event in self.session.drain() {
            self.handle_event(ctx, event);
        }
    }

    fn handle_event(&mut self, ctx: &Context, event: Event) {
        let Some(event) = self.handle_local_event(event) else {
            return;
        };
        match event {
            Event::RevealVoice { model, language } => {
                self.open_settings(Some(Tab::Model(model)));
                let fold = crate::settings_window::voice_fold(model, language);
                crate::settings_window::reveal(ctx, fold);
            }
            Event::Export {
                name,
                format,
                contents,
            } => self.ui.files.save(ctx, name, format, contents),
            Event::WordListWindow { pane, pages, list } => {
                if let Some(ask) = self.session.word_ask(pane, pages, list) {
                    let chapters = !ask.document.outline().is_empty();
                    self.ui.word_window.open(ctx, pane, ask, chapters);
                }
            }
            Event::FocusEdgeTab => {
                ctx.memory_mut(|m| m.request_focus(egui::Id::new(EDGE_TAB)));
                crate::focus::by_key(ctx);
            }
            Event::ReleaseFocus => {
                ctx.memory_mut(|m| m.focused().map(|id| m.surrender_focus(id)));
            }
            Event::PageAi(action) => self.page_ai_act(ctx, action),
            Event::SkillLanded(id) => self.skill_landed(id),
            Event::SkillShow(id) => self.ui.skill_reveal = Some(id),
            Event::Library(action) => self.library_action(ctx, action),
            Event::WordList { made, list } => self.deliver_word_list(ctx, &made, list.output),
            Event::UpdateRestart => {
                self.flush();
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            _ => {}
        }
    }

    /// Handles context-free events; returns `Some(event)` when ctx is needed.
    fn handle_local_event(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::Toast(toast) => self.ui.toasts.push(toast),
            Event::RouteChanged(route) => self.arrived(route),
            Event::OpenSettings(tab) => self.open_settings(Some(tab)),
            Event::CloseSettings => self.ui.settings_window.close(),
            Event::PaneOpened {
                pane,
                page,
                count,
                anchor,
            } => {
                self.ui
                    .documents
                    .on_opened(&mut self.session, pane, page, count, anchor);
            }
            Event::PaneClosed(pane) => {
                self.ui.documents.on_closed(pane);
                self.ui.page_ai.close_pane(pane);
            }
            Event::ForgetPages(doc) => self.ui.documents.forget_pages(doc),
            Event::PaneView { pane, action, now } => self.handle_pane_view(pane, action, now),
            other => return self.handle_local_ui_event(other),
        }
        None
    }

    fn handle_pane_view(&mut self, pane: PaneId, action: PaneAction, now: f64) {
        match action {
            PaneAction::Zoom(zoom) => self.ui.documents.zoom(&mut self.session, pane, zoom, now),
            PaneAction::SetMode(mode) => {
                self.ui
                    .documents
                    .apply_mode(&mut self.session, pane, mode, now);
            }
            PaneAction::Arrow(arrow) => {
                self.ui.documents.arrow(&mut self.session, pane, arrow, now);
            }
            PaneAction::PageKey { forward } => {
                self.ui.documents.page_key(&mut self.session, pane, forward);
            }
            PaneAction::ToggleDrift => {
                self.ui.documents.toggle_drift(&mut self.session, pane, now);
            }
            PaneAction::GoTo(page) => self.ui.documents.go_to(&mut self.session, pane, page),
        }
    }

    fn handle_local_ui_event(&mut self, event: Event) -> Option<Event> {
        match event {
            Event::PlaceReadMarker { pane, spot, now } => {
                self.ui.documents.place_read_marker(pane, spot, now);
            }
            Event::DismissReadMarkers(now) => {
                self.ui.documents.dismiss_read_markers(now);
            }
            Event::ReadAloudStarted { fresh } => self.ui.read_aloud.on_started(fresh),
            Event::Study(action) => {
                if action == mareader_engine::study::Action::Close {
                    self.ui.dict.clear_stash();
                }
                self.ui.study.act(&mut self.session, action);
            }
            Event::ExplainOpened { pane, key, asked } => {
                self.ui.dict.escape();
                let rect = self
                    .documents()
                    .hit_box(pane, asked.page, asked.chars)
                    .unwrap_or(Rect::ZERO);
                self.ui
                    .study
                    .show_explained(pane, key, asked.word, asked.context, rect);
                self.probe_dict_for_study();
            }
            Event::LookUpOpened {
                pane,
                word,
                ticket,
                spans,
                asked,
            } => self.open_lookup(pane, word, ticket, &spans, asked),
            Event::DictWindowFeed(word) => {
                self.ui.dict.escape();
                self.ui.dict.feed(&word);
            }
            Event::NoteOpened {
                pane,
                id,
                made,
                spans,
            } => {
                let rect = self
                    .documents()
                    .spans_rect(pane, &spans)
                    .unwrap_or(Rect::ZERO);
                let (state, model) = (self.session.study(), self.session.library_model());
                self.ui
                    .study
                    .open_created_note(state, model, pane, id, made, rect);
            }
            Event::DictWindow(window) => self.dict_window(window),
            Event::Pursue(word) => self.pursue(&word),
            Event::Dict(action) => {
                if action == mareader_engine::dict::Action::CloseCard {
                    self.ui.study.clear_stash();
                }
                self.ui.dict.act(action);
            }
            #[cfg(not(target_arch = "wasm32"))]
            Event::Ai(job) => self.ui.ai.send(job),
            Event::Role { word, role } => {
                self.ui.dict.set_role(&word, role.clone());
                self.ui.study.set_role(&word, role);
            }
            Event::DictReplies { changed, replies } => {
                let exact = !self.settings().dictionary.near;
                self.ui.dict.set_exact_only(exact);
                self.ui.dict.receive(changed, replies);
            }
            needs_ctx => return Some(needs_ctx),
        }
        None
    }

    /// Handles context-free events immediately and defers ctx-needing ones.
    pub(super) fn drain_local(&mut self) {
        for event in self.session.drain() {
            if let Some(deferred) = self.handle_local_event(event) {
                self.ui.deferred_events.push(deferred);
            }
        }
    }

    fn open_lookup(
        &mut self,
        pane: PaneId,
        word: String,
        ticket: mareader_dict::Ticket,
        spans: &[(usize, std::ops::Range<usize>)],
        asked: Option<mareader_engine::study::words::Asked>,
    ) {
        self.ui.study.escape(&mut self.session);
        let rect = self
            .documents()
            .spans_rect(pane, spans)
            .unwrap_or(Rect::ZERO);
        let offset = self.documents().offset(pane);
        self.ui
            .dict
            .show_lookup(pane, word, ticket, rect, offset, asked);
    }

    fn chrome(&mut self, ctx: &Context, ui: UiCommand) {
        match ui {
            UiCommand::ToggleSidebar => self.ui.sidebar.toggle(),
            UiCommand::OpenSidebar(panel) => self.ui.sidebar.open(panel),
            UiCommand::CloseSidebar => self.ui.sidebar.close(),
            UiCommand::ShowOpenDialog(placement) => self.ui.files.pick(ctx, placement),
            UiCommand::LibraryDialog(dialog) => self.library_dialog(ctx, dialog),
            UiCommand::Chromeless => self.toggle_chromeless(),
            UiCommand::Window(w) => ctx.send_viewport_cmd(w.viewport(ctx)),
            UiCommand::Notify(toast) => self.ui.toasts.push(toast),
            UiCommand::CloseToast(id) => self.ui.toasts.close(id, ctx.input(|i| i.time)),
            UiCommand::ClearToasts => self.ui.toasts.clear(ctx.input(|i| i.time)),
            UiCommand::WordCardMode(mode) => self.word_card_mode(mode),
            UiCommand::WordCardSwapLang => {
                if let Some(query) = self.ui.dict.swap_query() {
                    let ticket = self.session.dict_ask(query);
                    self.ui.dict.set_card_ticket(ticket);
                }
            }
            UiCommand::WordCardTryLang(lang) => {
                if let Some(query) = self.ui.dict.try_lang_query(&lang) {
                    let ticket = self.session.dict_ask(query);
                    self.ui.dict.set_card_ticket(ticket);
                }
            }
            other => self.reveal(ctx, other),
        }
    }

    /// The menus, windows and pages a command brings up.
    fn reveal(&mut self, ctx: &Context, ui: UiCommand) {
        match ui {
            UiCommand::ShowAppearance => {
                self.ui.find.close();
                self.ui.titlebar.reveal();
                egui::Popup::open_id(ctx, egui::Id::new(crate::titlebar::APPEARANCE_POPOVER));
            }
            UiCommand::ShowViewMenu => {
                self.ui.find.close();
                self.show_view_menu(ctx);
            }
            UiCommand::ShowSettings => self.open_settings(None),
            UiCommand::ShowShortcuts => {
                self.ui.find.close();
                crate::shortcuts::toggle(ctx);
            }
            UiCommand::ShowPalette => {
                self.ui.find.close();
                crate::palette::toggle(ctx);
            }
            UiCommand::CloseSettings => self.ui.settings_window.close(),
            UiCommand::FindStep { forward } => self.find_step(ctx, forward),
            UiCommand::ShowFind => self.show_find(ctx),
            UiCommand::ShowSearch if self.ui.find.is_open() => self.ui.find.close(),
            UiCommand::ShowSearch => self.show_find(ctx),
            UiCommand::ShowDictionary => self.show_dictionary(),
            UiCommand::ShowReading => self.show_reading(ctx),
            UiCommand::ShowLibrary => self.show_library(),
            _ => {}
        }
    }

    fn toggle_chromeless(&mut self) {
        self.ui.chromeless = !self.ui.chromeless;
        if self.ui.chromeless {
            self.ui.sidebar.close();
            let keys = keymap::hint(self.ui.platform, &Sent::Ui(UiCommand::Chromeless));
            let keys = keys.map_or(String::new(), |k| format!(" or {k}"));
            let how = format!("Distraction-free. Press Esc{keys} to leave.");
            self.ui.toasts.push(Toast::info(how));
        }
    }

    fn show_view_menu(&mut self, ctx: &Context) {
        self.ui.titlebar.reveal();
        let menu = match self.route() {
            Route::Reader => crate::view_menu::VIEW_MENU,
            Route::Library => crate::library::menu::LIBRARY_VIEW_MENU,
            Route::Dictionary | Route::Space => crate::titlebar::DICTIONARY_MENU,
        };
        egui::Popup::open_id(ctx, egui::Id::new(menu));
    }

    /// F3 and Cmd+G: the next match, opening the bar if it is shut.
    fn find_step(&mut self, ctx: &Context, forward: bool) {
        if !self.ui.find.is_open() {
            self.show_find(ctx);
            return;
        }
        let docs = self.ui.documents.as_view(self.session.documents());
        let found = found_in(self.session.search(), docs, &self.ui.find);
        self.ui.find.key_step(found, forward, ctx.input(|i| i.time));
    }

    /// With nothing to search in, find searches the library.
    fn show_find(&mut self, ctx: &Context) {
        match (self.route(), self.documents().reading(self.active())) {
            // The page's own field is its find.
            (Route::Dictionary, _) => self.ui.dict.arrive(),
            (Route::Reader, Some(_)) => {
                egui::Popup::close_all(ctx);
                self.ui.find.open(self.active());
                self.ui.titlebar.reveal();
            }
            _ => {
                self.go(Route::Library);
                let focus = mareader_engine::library::Action::Search(
                    mareader_engine::library::SearchAction::Focus,
                );
                self.apply(ctx, Sent::Engine(Command::Library(focus)));
            }
        }
    }

    fn show_dictionary(&mut self) {
        match self.route() {
            Route::Dictionary => self.go(self.dict_back()),
            // Over the page, unless Settings ask for the page.
            Route::Reader if self.settings().dictionary.opens == DictionaryOpens::Window => {
                self.dict_window(mareader_engine::dict::Window::Toggle);
            }
            _ => self.go(Route::Dictionary),
        }
    }

    /// Back to the pane reading aloud, turned to the words read.
    fn show_reading(&mut self, ctx: &Context) {
        if let Some((pane, page, _)) = self.session.read_aloud().spot() {
            if self.route() != Route::Reader {
                self.go(Route::Reader);
            }
            self.focus(pane);
            self.ui.documents.go_to(&mut self.session, pane, page);
        } else if let Some(origin) = self.session.read_aloud().away().cloned() {
            self.reopen_reading(ctx, origin);
        }
    }

    fn show_library(&mut self) {
        match self.route() {
            Route::Reader | Route::Dictionary | Route::Space => self.go(Route::Library),
            // An empty reader is no place to go back to.
            Route::Library if self.reading() => self.go(Route::Reader),
            Route::Library => {}
        }
    }
}
