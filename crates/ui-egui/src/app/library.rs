use super::*;

use crate::ui_command::{LibraryDialog, UiCommand};
use mareader_engine::command::Command;
use mareader_engine::library::Action as A;
use mareader_engine::library::SearchAction as S;

impl MareaderApp {
    /// Opens a file or folder picker dialog for a library action.
    pub(super) fn library_dialog(&mut self, ctx: &Context, dialog: LibraryDialog) {
        match dialog {
            LibraryDialog::Folder => {
                let ask = |root| Sent::Engine(Command::Library(A::AskFolder(root)));
                self.ui.files.pick_folder(ctx, "Import a folder", ask);
            }
            LibraryDialog::SearchFolder(book) => {
                let seek = move |folder| Sent::Engine(Command::Library(A::Seek { book, folder }));
                self.ui.files.pick_folder(ctx, "Search a folder", seek);
            }
            LibraryDialog::Import => {
                let to = self.session.library().at();
                self.pick_import(ctx, None, to);
            }
            LibraryDialog::From(dir) => {
                let to = self.session.library().at();
                self.pick_import(ctx, Some(dir), to);
            }
            LibraryDialog::Into(to) => self.pick_import(ctx, None, to),
            LibraryDialog::File(book) => {
                let Some(format) = self.session.library_model().book(book).map(|b| b.format) else {
                    return;
                };
                let relink = move |path| Sent::Engine(Command::Library(A::Relink { book, path }));
                self.ui.files.pick_book(ctx, format.extensions(), relink);
            }
        }
    }

    /// Library work that needs front-end focus, popups, or the AI service.
    pub(super) fn library_action(
        &mut self,
        ctx: &Context,
        action: mareader_engine::library::Action,
    ) {
        match action {
            A::Search(S::Focus) => {
                self.ui.titlebar.reveal();
                ui_library::search::focus(ctx);
            }
            A::Search(S::Choose(_) | S::ChooseLit) => ui_library::search::blur(ctx),
            A::ShowAdd => egui::Popup::open_id(ctx, egui::Id::new(ui_library::ADD_MENU)),
            A::Organize(picked) => self.organize(picked, false),
            A::OrganizeAnew(picked) => self.organize(picked, true),
            A::ResumeOrganizing => self.resume_organizing(),
            A::StopOrganizing => {
                self.ui.ai.stop_organizing();
                // What was done is kept; its card offers to carry on.
                self.session.library_set_paused(self.ui.ai.paused());
                self.session.library_organizer_end();
            }
            A::DiscardPaused => self.discard_paused_organize(),
            A::RestorePaused => {
                self.ui.ai.bring_back();
                self.session.library_set_paused(self.ui.ai.paused());
            }
            A::ApplyOrganized => self.ui.ai.forget(),
            A::DiscardOrganized if self.ui.ai.set_aside() => {
                let undo = Command::Library(A::RestorePaused);
                self.ui
                    .toasts
                    .push(Toast::info("Discarded the shelf plan.").undo(undo));
            }
            _ => {}
        }
    }

    fn discard_paused_organize(&mut self) {
        let ready = self
            .session
            .library()
            .organizer()
            .paused()
            .is_some_and(|p| p.ready);
        if self.ui.ai.set_aside() {
            let said = match ready {
                true => "Discarded the shelf plan.",
                false => "Discarded the paused run.",
            };
            let undo = Command::Library(A::RestorePaused);
            self.ui.toasts.push(Toast::info(said).undo(undo));
        }
        self.session.library_set_paused(None);
    }

    /// The last three books opened, rebuilt only as the library changes.
    pub(super) fn recent_books(&mut self) -> Vec<(mareader_library::BookId, String)> {
        let model = self.session.library_model();
        if self.ui.reopen.0 != model.revision() {
            let books = model.recent().into_iter();
            let opened = books.filter(|b| b.opened.value > 0 && !b.missing());
            let three = opened.take(3).map(|b| (b.id, b.title.value.clone()));
            self.ui.reopen = (model.revision(), three.collect());
        }
        self.ui.reopen.1.clone()
    }

    /// Asks the AI to shelve loose books; a paused run asks first.
    pub(super) fn organize(&mut self, picked: Option<Vec<mareader_library::BookId>>, anew: bool) {
        self.session.library_close_replace();
        if !anew && self.session.library().organizer().paused().is_some() {
            return self.session.library_ask_replace(picked);
        }
        match self.session.library().ask_organize(picked.as_deref()) {
            Ok(ask) => self.start_organizing(&ask, false),
            Err(why) => self.ui.toasts.push(Toast::warning(why)),
        }
    }

    /// Carries on the paused run; one with nothing left is let go.
    pub(super) fn resume_organizing(&mut self) {
        self.session.library_close_replace();
        match self.session.library().ask_resume() {
            Ok(ask) => self.start_organizing(&ask, true),
            Err(why) => {
                if !self.session.library().busy() {
                    self.ui.ai.forget();
                    self.session.library_set_paused(None);
                }
                self.ui.toasts.push(Toast::warning(why));
            }
        }
    }

    /// Runs `ask`; a finished paused run needs no model to show its plan.
    pub(super) fn start_organizing(&mut self, ask: &mareader_engine::library::Ask, resume: bool) {
        let ai = &self.settings().ai;
        let ready = resume
            && self
                .session
                .library()
                .organizer()
                .paused()
                .is_some_and(|p| p.ready);
        let organizer = ai.model(mareader_settings::TaskId::Organize);
        let model = organizer.map(mareader_settings::ModelPreset::title);
        let Some(model) = model.or_else(|| ready.then(|| "plan ready".to_owned())) else {
            let why = "Choose a model to organize with in Settings › AI.";
            return self.ui.toasts.push(Toast::warning(why));
        };
        #[cfg(not(target_arch = "wasm32"))]
        if self.ui.ai.organize(ask, ai.tasks.organize.batch, resume) {
            self.session
                .library_organizing(ask.books.iter().map(|b| b.id), &model);
            // A busy model says so at once.
            self.take_organized();
            return;
        }
        let _ = (ask, model, resume);
        self.ui
            .toasts
            .push(Toast::warning("The AI isn't available here."));
    }

    /// The run's news, into the library; an ended run's steps are read back.
    pub(super) fn take_organized(&mut self) {
        let heard = self.ui.ai.organized();
        use mareader_engine::library::Heard;
        let over = |h: &Heard| !matches!(h, Heard::Progress { .. });
        let empty = |h: &Heard| matches!(h, Heard::Plan(p) if p.placed.is_empty());
        if heard.iter().any(empty) {
            // A plan that moves nothing has nothing to review later.
            self.ui.ai.forget();
            self.session.library_set_paused(None);
        } else if heard.iter().any(over) {
            self.session.library_set_paused(self.ui.ai.paused());
        }
        let mut out = Commands::new();
        for heard in heard {
            let asked = self.session.library_heard(heard);
            ui_library::push_out(asked, &mut out);
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
            let action = mareader_engine::library::BookAction::Import { paths, to };
            Sent::Engine(Command::Library(mareader_engine::library::Action::Book(
                action,
            )))
        };
        self.ui.files.pick_books(ctx, from, import);
    }
}

/// The shelf's own key: the page the shelf is read on.
///
/// `None` for a key that is not one of them, so the next family is asked.
pub(super) fn own_key(app: &mut MareaderApp, key: &str, value: &str) -> Option<Result<(), String>> {
    match key {
        "library" => Some(app.option_library(key, value)),
        _ => None,
    }
}

impl MareaderApp {
    fn option_library(&mut self, _key: &str, value: &str) -> Result<(), String> {
        match value {
            // After a queued open, which would otherwise leave it.
            "show"
                if self
                    .ui
                    .queued
                    .iter()
                    .any(|c| matches!(c, Sent::Engine(Command::Open(_)))) =>
            {
                self.queue(Sent::Ui(UiCommand::ShowLibrary));
            }
            "show" => self.go(Route::Library),
            _ => self.go(Route::Reader),
        };
        Ok(())
    }
}
