use super::*;

use ui_library::Action as A;
use ui_library::SearchAction as S;

impl MareaderApp {
    /// Library work; its dialogs need the app's files.
    pub(super) fn library_action(&mut self, ctx: &Context, action: ui_library::Action) {
        if library_handles_1(&action) {
            self.library_special_1(ctx, action);
            return;
        }
        if library_handles_2(&action) {
            self.library_special_2(ctx, action);
            return;
        }
        if library_handles_3(&action) {
            self.library_special_3(ctx, action);
            return;
        }
        let mut out = Commands::new();
        self.library.act(action, ctx.input(|i| i.time), &mut out);
        for command in out {
            self.apply(ctx, command);
        }
    }

    fn library_special_1(&mut self, ctx: &Context, action: ui_library::Action) {
        match action {
            A::Search(S::Focus) => {
                self.titlebar.reveal();
                ui_library::search::focus(ctx);
            }
            A::Search(S::Choose(_) | S::ChooseLit) => {
                ui_library::search::blur(ctx);
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                for command in out {
                    self.apply(ctx, command);
                }
            }
            A::ShowAdd => egui::Popup::open_id(ctx, egui::Id::new(ui_library::ADD_MENU)),
            A::PickFolder => {
                let ask = |root| Command::Library(A::AskFolder(root));
                self.files.pick_folder(ctx, "Import a folder", ask);
            }
            A::PickSearchFolder(book) => {
                let seek = move |folder| Command::Library(A::Seek { book, folder });
                self.files.pick_folder(ctx, "Search a folder", seek);
            }
            A::Book(ui_library::BookAction::PickImport) => {
                let to = self.library.at();
                self.pick_import(ctx, None, to);
            }
            A::PickFrom(dir) => {
                let to = self.library.at();
                self.pick_import(ctx, Some(dir), to);
            }
            A::PickInto(to) => self.pick_import(ctx, None, to),
            A::Book(ui_library::BookAction::Reveal(book)) => {
                let path = self.library.model().book(book).and_then(|b| b.path());
                let shown = path.is_some_and(crate::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.toasts.push(Toast::error(note));
                }
            }
            _ => {}
        }
    }

    fn library_special_2(&mut self, ctx: &Context, action: ui_library::Action) {
        match action {
            A::Shelf(ui_library::ShelfAction::Reveal(shelf)) => {
                let dir = self.library.model().shelf_dir(shelf);
                let shown = dir.as_deref().is_some_and(crate::reveal::reveal);
                if !shown {
                    let note = "The file manager couldn't be opened.";
                    self.toasts.push(Toast::error(note));
                }
            }
            A::PickFile(book) => {
                let Some(format) = self.library.model().book(book).map(|b| b.format) else {
                    return;
                };
                let relink = move |path| Command::Library(A::Relink { book, path });
                self.files.pick_book(ctx, format.extensions(), relink);
            }
            A::Organize(picked) => self.organize(picked, false),
            A::OrganizeAnew(picked) => self.organize(picked, true),
            A::ResumeOrganizing => self.resume_organizing(),
            A::StopOrganizing => {
                self.ai.stop_organizing();
                // What was done is kept; its card offers to carry on.
                self.library.set_paused(self.ai.paused());
                self.library.organizer_end();
            }
            A::DiscardPaused => {
                let ready = self.library.organizer().paused().is_some_and(|p| p.ready);
                if self.ai.set_aside() {
                    let said = match ready {
                        true => "Discarded the shelf plan.",
                        false => "Discarded the paused run.",
                    };
                    let undo = Command::Library(A::RestorePaused);
                    self.toasts.push(Toast::info(said).undo(undo));
                }
                self.library.set_paused(None);
            }
            A::RestorePaused => {
                self.ai.bring_back();
                self.library.set_paused(self.ai.paused());
            }
            _ => {}
        }
    }

    fn library_special_3(&mut self, ctx: &Context, action: ui_library::Action) {
        match action {
            A::ApplyOrganized | A::DiscardOrganized => {
                // Applied, the run is done; a discarded plan can come back.
                let undoable = match action {
                    A::DiscardOrganized => self.ai.set_aside(),
                    _ => {
                        self.ai.forget();
                        false
                    }
                };
                self.library.set_paused(None);
                let mut out = Commands::new();
                self.library.act(action, ctx.input(|i| i.time), &mut out);
                if undoable {
                    let undo = Command::Library(A::RestorePaused);
                    let said = "Discarded the shelf plan.";
                    out.push(Command::Notify(Toast::info(said).undo(undo)));
                }
                for command in out {
                    self.apply(ctx, command);
                }
            }
            _ => {}
        }
    }

    /// The last three books opened, rebuilt only as the library changes.
    pub(super) fn recent_books(&mut self) -> Vec<(mareader_library::BookId, String)> {
        let model = self.library.model();
        if self.reopen.0 != model.revision() {
            let books = model.recent().into_iter();
            let opened = books.filter(|b| b.opened.value > 0 && !b.missing());
            let three = opened.take(3).map(|b| (b.id, b.title.value.clone()));
            self.reopen = (model.revision(), three.collect());
        }
        self.reopen.1.clone()
    }

    /// Asks the AI to shelve loose books; a paused run asks first.
    pub(super) fn organize(&mut self, picked: Option<Vec<mareader_library::BookId>>, anew: bool) {
        self.library.close_replace();
        if !anew && self.library.organizer().paused().is_some() {
            return self.library.ask_replace(picked);
        }
        match self.library.ask_organize(picked.as_deref()) {
            Ok(ask) => self.start_organizing(&ask, false),
            Err(why) => self.toasts.push(Toast::info(why)),
        }
    }

    /// Carries on the paused run; one with nothing left is let go.
    pub(super) fn resume_organizing(&mut self) {
        self.library.close_replace();
        match self.library.ask_resume() {
            Ok(ask) => self.start_organizing(&ask, true),
            Err(why) => {
                if !self.library.organizer().busy() {
                    self.ai.forget();
                    self.library.set_paused(None);
                }
                self.toasts.push(Toast::info(why));
            }
        }
    }

    /// Runs `ask`; a finished paused run needs no model to show its plan.
    pub(super) fn start_organizing(&mut self, ask: &ui_library::organizing::Ask, resume: bool) {
        let ai = &self.settings.ai;
        let ready = resume && self.library.organizer().paused().is_some_and(|p| p.ready);
        let model = ai
            .organize_preset()
            .map(mareader_settings::ModelPreset::title);
        let Some(model) = model.or_else(|| ready.then(|| "plan ready".to_owned())) else {
            let why = "Choose a model to organize with in Settings › AI.";
            return self.toasts.push(Toast::warning(why));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.ai.organize(ask, ai.tasks.organize.batch, resume) {
            self.library
                .organizing(ask.books.iter().map(|b| b.id), &model);
            // A busy model says so at once.
            self.take_organized();
            return;
        }
        let _ = (ask, model, resume);
        self.toasts
            .push(Toast::warning("The AI isn't available here."));
    }

    /// The run's news, into the library; an ended run's steps are read back.
    pub(super) fn take_organized(&mut self) {
        let heard = self.ai.organized();
        use ui_library::organizing::Heard;
        let over = |h: &Heard| !matches!(h, Heard::Progress { .. });
        let empty = |h: &Heard| matches!(h, Heard::Plan(p) if p.placed.is_empty());
        if heard.iter().any(empty) {
            // A plan that moves nothing has nothing to review later.
            self.ai.forget();
            self.library.set_paused(None);
        } else if heard.iter().any(over) {
            self.library.set_paused(self.ai.paused());
        }
        let mut out = Commands::new();
        for heard in heard {
            self.library.heard(heard, &mut out);
        }
        for command in out {
            self.queue(command);
        }
    }

    /// The file dialog for books, landing on `to` (none: the top).
    pub(super) fn pick_import(
        &mut self,
        ctx: &Context,
        from: Option<PathBuf>,
        to: Option<mareader_library::ShelfId>,
    ) {
        let import = move |paths| {
            let action = ui_library::BookAction::Import { paths, to };
            Command::Library(ui_library::Action::Book(action))
        };
        self.files.pick_books(ctx, from, import);
    }
}

fn library_handles_1(action: &ui_library::Action) -> bool {
    matches!(
        action,
        A::Search(S::Focus | S::Choose(_) | S::ChooseLit)
            | A::ShowAdd
            | A::PickFolder
            | A::PickSearchFolder(..)
            | A::Book(ui_library::BookAction::PickImport | ui_library::BookAction::Reveal(_))
            | A::PickFrom(..)
            | A::PickInto(..)
    )
}

fn library_handles_2(action: &ui_library::Action) -> bool {
    matches!(
        action,
        A::Shelf(ui_library::ShelfAction::Reveal(_))
            | A::PickFile(..)
            | A::Organize(..)
            | A::OrganizeAnew(..)
            | A::ResumeOrganizing
            | A::StopOrganizing
            | A::DiscardPaused
            | A::RestorePaused
    )
}

fn library_handles_3(action: &ui_library::Action) -> bool {
    matches!(action, A::ApplyOrganized | A::DiscardOrganized)
}
