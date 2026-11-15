use super::*;

impl MareaderApp {
    pub(super) fn prepare(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &Context,
        frame: &mut eframe::Frame,
    ) -> bool {
        self.note_focus();
        if self.documents.gpu().is_none()
            && let Some(render) = frame.wgpu_render_state()
        {
            self.documents.set_gpu(Gpu::new(render));
        }
        // Fonts from set_fonts arrive next frame; named families panic before.
        if self.fonts != FontState::Ready {
            self.fonts = FontState::Ready;
            // Still paper underneath, the frame the window first shows.
            let paper = theme::Tokens::get(ctx).paper;
            ui.painter().rect_filled(ctx.content_rect(), 0.0, paper);
            ctx.request_repaint();
            return false;
        }
        if let Some(after) = self.fade.poll(ctx) {
            self.restore(after);
            self.restyle(ctx);
            ctx.request_repaint_after(SAVE_EVERY + Duration::from_millis(50));
        }
        self.reconcile_looks(ctx);
        self.reconcile_texts();
        self.titlebar.pinned = self.settings.chrome.pinned(self.route().bar());
        resize::edges(ctx, self.platform.frameless());
        true
    }

    pub(super) fn commands(&mut self, ctx: &Context) -> (Commands, Route, bool) {
        // Tests, shots and the system change looks without a fade.
        let outside = !self.queued.is_empty();
        let mut commands = std::mem::take(&mut self.queued);
        let designed = self.book_designs();
        let mut own = self.texts.own(&self.settings.text);
        own.retain(|(pane, _)| designed.iter().all(|(p, _)| p != pane));
        own.extend(designed.iter().map(|(pane, text)| (*pane, text)));
        self.documents.set_text(&self.settings.text, &own);
        // Set anew, a book read aloud lands on the words heard.
        self.documents.hold_said(self.read_aloud.said());
        self.documents.receive(ctx, &mut commands);
        self.files.collect(&mut commands);
        self.library.receive(ctx, &mut commands);
        self.library.tick(ctx);
        // A closed book's search would keep it, and its thread, alive.
        let documents = &self.documents;
        self.search.forget_closed(|doc| documents.document(doc));
        if self.search.poll() {
            ctx.request_repaint();
        }
        // Inline, the measure lands next frame; book it.
        if self.library.observe(&self.documents, &self.tree.panes()) {
            ctx.request_repaint();
        }
        if let Some(wait) = self.library.save(ctx.input(|i| i.time)) {
            ctx.request_repaint_after(Duration::from_secs_f64(wait));
        }
        let route = self.route();
        // The library keeps its bar: it has no other way out.
        self.titlebar.chromeless = self.chromeless && route == Route::Reader;
        // Tab brings hidden chrome up, unless a dialog would sit below it.
        let dialog = self.settings_window.is_open()
            || self.word_window.is_open()
            || self.library.sheet_up()
            || crate::shortcuts::is_open(ctx);
        if crate::focus::begin(ctx) && !dialog {
            self.titlebar.reveal();
            self.page_bars.entry(self.active).or_default().reveal();
        }
        // Dialog answers precede scrolling and Escape, but never typing.
        let asking = route == Route::Reader
            && self.read_aloud.is_asking()
            && !dialog
            && !ctx.text_edit_focused()
            && !egui::Popup::is_any_open(ctx);
        if asking {
            let read = !ctx.egui_wants_keyboard_input()
                && ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Space));
            let skip = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            if read || skip {
                self.read_aloud
                    .answer_question(read, &mut self.settings.read_aloud);
            }
        }
        let read = commands.len();
        keymap::read(ctx, self.platform, route, &mut commands);
        // The shortcut gives an idle open window the keys before closing it.
        let win = &self.settings.dictionary.window;
        let floats = self.settings.dictionary.opens == DictionaryOpens::Window;
        if route == Route::Reader && floats && win.open && !win.minimized && !self.dict.typing() {
            let focus = Command::Dict(crate::dict::Action::Window(crate::dict::Window::Focus));
            for c in commands.iter_mut().skip(read) {
                if matches!(c, Command::ShowDictionary) {
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
        let modal = self.settings_window.is_open()
            || self.word_window.is_open()
            || self.library.sheet_up()
            || self.library.renaming().is_some()
            || (route == Route::Library && ui_library::search::focused(ctx))
            || crate::image_viewer::is_open(ctx)
            || crate::shortcuts::is_open(ctx)
            || self.lifting.is_some();
        let mut free = escape && !modal && !egui::Popup::is_any_open(ctx);
        // The library page lets go of a selection, else goes back to reading.
        if free && route == Route::Library {
            if !self.library.escape() {
                commands.push(Command::ShowLibrary);
            }
            free = false;
        }
        // The dictionary clears its search, else goes back.
        if free && route == Route::Dictionary {
            if !self.dict.page_escape() {
                commands.push(Command::ShowDictionary);
            }
            free = false;
        }
        // Typing in the floating window: clear, then close.
        let window = self.settings.dictionary.window.open && route == Route::Reader;
        if free && window && self.dict.typing() {
            if !self.dict.page_escape() {
                commands.push(Command::Dict(crate::dict::Action::Window(
                    crate::dict::Window::Close,
                )));
            }
            free = false;
        }
        if free && (self.dict.escape() || self.study.escape(self.library.model_mut())) {
            free = false;
        }
        if free && self.documents.dismiss_read_markers(ctx.input(|i| i.time)) {
            free = false;
        }
        if free && self.documents.stop_drift(self.active) {
            free = false;
        }
        if free && self.find.is_open() {
            self.find.close();
            free = false;
        }
        // A selection, then picked pages, before the sidebar and chromeless.
        let selection = free && self.documents.clear_selection(self.active);
        let grid = self.sidebar.is_open() && self.sidebar.panel() == Panel::Thumbnails;
        if free && !selection && !(grid && self.thumbs.clear_picks()) {
            if self.sidebar.is_open() {
                commands.push(Command::CloseSidebar);
            } else if self.chromeless {
                commands.push(Command::Chromeless);
            }
        }
        drop_target
    }
}
