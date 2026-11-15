use super::*;
use crate::ui_command::UiCommand;
use mareader_engine::command::Command;

impl MareaderApp {
    pub(super) fn prepare(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        frame: &mut eframe::Frame,
    ) -> bool {
        self.note_focus();
        if self.ui.documents.gpu().is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.ui.documents.set_gpu(Gpu::new(render));
        }
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.ui.fonts != FontState::Ready {
            self.ui.fonts = FontState::Ready;
            // Still paper underneath, the frame the window first shows.
            let paper = theme::Tokens::get(ctx).paper;
            ui.painter().rect_filled(ctx.content_rect(), 0.0, paper);
            ctx.request_repaint();
            return false;
        }
        if let Some(after) = self.ui.fade.poll(ctx) {
            self.restore(after);
            self.restyle(ctx);
            ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
        }
        self.reconcile_looks(ctx);
        self.reconcile_texts();
        resize::edges(ctx, self.ui.platform.frameless());
        true
    }

    pub(super) fn commands(&mut self, ctx: &Context) -> (Commands, Route, bool) {
        // Tests, shots and the system change looks without a fade.
        let outside = !self.ui.queued.is_empty();
        let mut commands = std::mem::take(&mut self.ui.queued);
        self.ui.documents.sync_text(&mut self.session);
        self.ui
            .documents
            .receive(ctx, &mut self.session, &mut commands);
        self.ui.files.collect(&mut commands);
        self.tick_session(ctx);
        self.tick_library(ctx);
        self.keep(ctx, ctx.input(|i| i.time));
        let route = self.route();
        // The library keeps its bar: it has no other way out.
        self.ui.titlebar.chromeless = self.ui.chromeless && route == Route::Reader;
        // Tab brings hidden chrome up, unless a dialog would sit below it.
        let dialog = self.ui.settings_window.is_open()
            || self.ui.word_window.is_open()
            || self.session.library().sheet_up()
            || crate::shortcuts::is_open(ctx)
            || crate::palette::is_open(ctx);
        if crate::focus::begin(ctx) && !dialog {
            self.ui.titlebar.reveal();
            self.ui.page_bars.entry(self.active()).or_default().reveal();
        }
        // Dialog answers precede scrolling and Escape, but never typing.
        let asking = route == Route::Reader
            && self.session.read_aloud().is_asking()
            && !dialog
            && !ctx.text_edit_focused()
            && !egui::Popup::is_any_open(ctx);
        if asking {
            let read = !ctx.egui_wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
            let skip = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if read || skip {
                let remember = self.ui.read_aloud.take_answer_remember();
                if !read
                    && let (Some(book), Some(kind)) = (
                        self.session.read_aloud().origin().and_then(|o| o.book),
                        remember,
                    )
                {
                    let mut settings = self.settings().read_aloud.clone();
                    mareader_engine::read_aloud::state::remember_skip(&mut settings, book, kind);
                    let _ = self.set_field("read_aloud", settings);
                }
                self.send(Command::ReadAloudAct(
                    mareader_engine::read_aloud::player::Act::Answer(read),
                ));
            }
        }
        let read = commands.len();
        keymap::read(ctx, self.ui.platform, route, &mut commands);
        // The shortcut gives an idle open window the keys before closing it.
        let win = &self.settings().dictionary.window;
        let floats = self.settings().dictionary.opens == DictionaryOpens::Window;
        if route == Route::Reader && floats && win.open && !win.minimized && !self.ui.dict.typing()
        {
            let focus = Sent::Engine(Command::Dict(mareader_engine::dict::Action::Window(
                mareader_engine::dict::Window::Focus,
            )));
            for c in commands.iter_mut().skip(read) {
                if matches!(c, Sent::Ui(UiCommand::ShowDictionary)) {
                    *c = focus.clone();
                }
            }
        }
        (commands, route, outside)
    }

    pub(super) fn escape(
        &mut self,
        ctx: &Context,
        route: Route,
        commands: &mut Commands,
    ) -> Option<PaneId> {
        let drop_target = self.drops(ctx, commands);
        // An open popup takes Escape first.
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        // Naming a shelf takes Escape to cancel.
        let modal = self.ui.settings_window.is_open()
            || self.ui.word_window.is_open()
            || self.session.library().sheet_up()
            || self.session.library().renaming().is_some()
            || (route == Route::Library && ui_library::search::focused(ctx))
            || crate::image_viewer::is_open(ctx)
            || crate::shortcuts::is_open(ctx)
            || crate::palette::is_open(ctx)
            || self.ui.lifting.is_some();
        let mut free = escape && !modal && !egui::Popup::is_any_open(ctx);
        // The library page lets go of a selection, else goes back to reading.
        if free && route == Route::Library {
            self.escape_library(commands);
            free = false;
        }
        // The dictionary clears its search, else goes back.
        if free && route == Route::Dictionary {
            if !self.ui.dict.page_escape() {
                commands.push(Sent::Ui(UiCommand::ShowDictionary));
            }
            free = false;
        }
        // Typing in the floating window: clear, then close.
        let window = self.settings().dictionary.window.open && route == Route::Reader;
        if free && window && self.ui.dict.typing() {
            if !self.ui.dict.page_escape() {
                commands.push(Sent::Engine(Command::Dict(
                    mareader_engine::dict::Action::Window(mareader_engine::dict::Window::Close),
                )));
            }
            free = false;
        }
        if free && (self.ui.dict.escape() || self.ui.study.escape(&mut self.session)) {
            free = false;
        }
        if free
            && self
                .ui
                .documents
                .dismiss_read_markers(ctx.input(|i| i.time))
        {
            free = false;
        }
        if free && self.ui.documents.stop_drift(self.active()) {
            free = false;
        }
        if free && self.ui.find.is_open() {
            self.ui.find.close();
            free = false;
        }
        // A selection, then picked pages, before the sidebar and chromeless.
        let now = ctx.input(|i| i.time);
        let clear = Command::ClearSelection(self.active());
        let selection = free && self.session.execute(clear, now).dirty;
        let grid = self.ui.sidebar.is_open() && self.ui.sidebar.panel() == Panel::Thumbnails;
        if free && !selection && !(grid && self.ui.thumbs.clear_picks()) {
            if self.ui.sidebar.is_open() {
                commands.push(Sent::Ui(UiCommand::CloseSidebar));
            } else if self.ui.chromeless {
                commands.push(Sent::Ui(UiCommand::Chromeless));
            }
        }
        drop_target
    }

    fn escape_library(&mut self, commands: &mut Commands) {
        if self.session.library().picks().active() {
            let clear = mareader_engine::library::Action::Pick(mareader_library::PickAction::Clear);
            self.send(Command::Library(clear));
        } else if !self.session.library().search_text().is_empty() {
            let clear = mareader_engine::library::Action::Search(
                mareader_engine::library::SearchAction::Clear,
            );
            self.send(Command::Library(clear));
        } else {
            commands.push(Sent::Ui(UiCommand::ShowLibrary));
        }
    }

    fn tick_library(&mut self, ctx: &Context) {
        let (now, focused) = ctx.input(|i| (i.time, i.focused));
        if let Some(wait) = self.session.library_tick(now, focused) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        let docs = &self.ui.documents;
        let measuring = self
            .session
            .library_observe(|pane, place| docs.view_anchor(pane, place));
        // Inline, the measure lands next frame; book it.
        if measuring {
            ctx.request_repaint();
        }
    }
}
