use super::*;
use crate::app::navigation::found_in;

impl MareaderApp {
    /// The owner of everything that outlives a frame.
    pub fn session(&self) -> &mareader_engine::session::Session {
        &self.session
    }

    pub fn settings(&self) -> &Settings {
        self.session.settings()
    }

    pub fn tree(&self) -> &mareader_workspace::PaneTree {
        self.session.tree()
    }

    /// The focused pane.
    pub fn active(&self) -> PaneId {
        self.session.active()
    }

    /// Makes `pane` the focused pane.
    pub(super) fn focus(&mut self, pane: PaneId) {
        self.send(Command::FocusPane(pane));
    }

    /// The panes by last focus, latest first.
    pub fn recent(&self) -> &[PaneId] {
        self.session.recent()
    }

    /// Takes a whole new settings, through the door.
    ///
    /// The settings window's own command, and a fixture's: what a shot
    /// changes arrives as one write, so no dial is set behind the door.
    pub fn set_settings(&mut self, settings: Settings) {
        self.send(Command::SetSettings(Box::new(settings)));
    }

    /// One dial, by its dotted path, to whatever JSON `value` makes.
    ///
    /// The option handlers' shared way in: a path names the setting, so a
    /// new dial needs no new command.
    pub(super) fn set_field<T: serde::Serialize>(
        &mut self,
        path: &str,
        value: T,
    ) -> Result<(), String> {
        let value = serde_json::to_string(&value).map_err(|e| e.to_string())?;
        let path = path.into();
        let done = self.session.execute(Command::SetField { path, value }, 0.0);
        done.refused.map_or(Ok(()), Err)
    }

    /// A command the session runs alone: a dial, the focus, or the route.
    pub(super) fn send(&mut self, intent: Command) {
        self.session.execute(intent, 0.0);
        self.drain_local();
    }

    pub fn read_aloud(&self) -> crate::read_aloud::ReadAloudView<'_> {
        crate::read_aloud::ReadAloudView::new(self.session.read_aloud(), &self.ui.read_aloud)
    }

    /// The file being read: the active pane's, else the first with one.
    pub fn last_document(&self) -> Option<LastDocument> {
        self.session
            .last_document_with(|p, at| self.documents().pane_view(p)?.place(at))
    }

    pub fn library(&self) -> &mareader_library::Library {
        self.session.library_model()
    }

    /// The page's own state: the open shelf, a name being edited.
    pub fn library_state(&self) -> LibraryView<'_> {
        LibraryView::new(self.session.library(), self.ui.library.covers())
    }

    /// Records a word directly in the library, for test fixtures.
    pub fn seed_library_word(
        &mut self,
        book: mareader_library::BookId,
        word: mareader_library::NewWord,
    ) -> Option<mareader_library::WordId> {
        self.session.library_seed_word(book, word)
    }

    /// Creates a shelf directly on the library model, for test fixtures.
    pub fn seed_library_shelf(
        &mut self,
        name: &str,
    ) -> Result<mareader_library::ShelfId, mareader_library::ShelfError> {
        self.session.library_seed_shelf(name)
    }

    /// Creates and names a link at the open shelf, for test fixtures.
    pub fn seed_library_link(
        &mut self,
        target: mareader_library::LinkTo,
        name: &str,
    ) -> Option<mareader_library::LinkId> {
        self.session.library_seed_link(target, name)
    }

    /// Sets the paused AI organizer state, for test fixtures.
    pub fn set_library_paused(&mut self, paused: Option<mareader_engine::library::Paused>) {
        self.session.library_set_paused(paused);
    }

    /// Feeds an AI organizer update into the library, for test fixtures.
    pub fn library_heard(&mut self, heard: mareader_engine::library::Heard, out: &mut Commands) {
        ui_library::push_out(self.session.library_heard(heard), out);
    }

    /// The book `pane` reads as: its own, else its file's.
    pub(super) fn book_in(&self, pane: PaneId) -> Option<mareader_library::BookId> {
        self.session.book_in(pane)
    }

    pub fn library_showing(&self) -> bool {
        self.route() == Route::Library
    }

    /// Covers held as textures, for tests.
    /// Document searches kept, for meters.
    pub fn searches(&self) -> usize {
        self.session.search().len()
    }

    pub fn covers_shown(&self) -> usize {
        self.ui.library.covers().shown()
    }

    /// The newest toast still up.
    pub fn toast(&self) -> Option<&Toast> {
        self.ui.toasts.current()
    }

    /// Every toast still up, newest first, and whether the stack is open.
    pub fn toasts(&self) -> (Vec<&Toast>, bool) {
        (self.ui.toasts.shown().collect(), self.ui.toasts.is_open())
    }

    /// Applied at the end of the next frame, like a view's commands.
    pub fn queue(&mut self, command: Sent) {
        self.ui.queued.push(command);
    }

    /// Store true here, from any thread, when the system wants less motion.
    pub fn reduced_motion(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.ui.reduced_motion)
    }

    pub fn find(&self) -> &FindBar {
        &self.ui.find
    }

    /// What the find bar's query has found in its pane, if asked yet.
    pub fn found(&self) -> Option<&Found> {
        found_in(self.session.search(), self.documents(), &self.ui.find)
    }

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.ui.settings_window
    }

    pub fn sidebar(&self) -> &Sidebar {
        &self.ui.sidebar
    }

    /// Where the macOS traffic lights go; the desktop binary moves them.
    pub fn native_lights(&self) -> NativeLights {
        self.ui.lights
    }

    /// Where the binary found the lights after moving them; none when hidden.
    pub fn set_light_spots(&mut self, spots: Option<LightSpots>) {
        self.ui.light_spots = spots;
    }

    pub fn titlebar(&self) -> &TitleBar {
        &self.ui.titlebar
    }

    /// Whether the current route's bar is pinned. The settings own it, so this
    /// is the only place to ask; there is no copy to fall out of step.
    pub fn pinned(&self) -> bool {
        self.settings().chrome.pinned(self.route().bar())
    }

    /// Whether the title bar is on screen: pinned, or hovered in.
    pub fn bar_shown(&self) -> bool {
        self.ui.titlebar.visible(self.pinned())
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree().panes()
    }

    /// Each pane's rect as last drawn.
    pub fn pane_rects(&self) -> &[(PaneId, egui::Rect)] {
        &self.ui.pane_rects
    }

    /// Whether a theme change is fading in over the old look.
    pub fn fading(&self) -> bool {
        self.ui.fade.is_fading()
    }

    pub fn active_pane(&self) -> PaneId {
        self.session.active()
    }

    pub fn documents(&self) -> DocumentsView<'_> {
        self.ui.documents.as_view(self.session.documents())
    }

    /// Dictionary service and window overlays, read-only.
    pub fn dict(&self) -> crate::dict::DictView<'_> {
        crate::dict::DictView::new(self.session.dict(), &self.ui.dict)
    }

    pub fn dict_mut(&mut self) -> &mut crate::dict::DictUi {
        &mut self.ui.dict
    }

    /// Whether the active pane's scripts have run and taken every click.
    pub fn scripts_settled(&self) -> bool {
        let pane = self.documents().scripts_idle(self.active());
        self.ui.script_shot.is_empty() && pane && self.ui.scripts.idle()
    }

    /// The scripts' state, for a screenshot that waited too long.
    pub fn scripts_state(&self) -> String {
        let pane = self.documents().scripts_state(self.active());
        format!("{}; {pane}", self.ui.scripts.describe())
    }

    /// The pronunciation pack, for tests and shots.
    pub fn sounds(&self) -> &mareader_engine::sounds::Sounds {
        self.session.sounds()
    }

    /// The word levels: the dataset and the shaded words.
    pub fn vocab(&self) -> &mareader_engine::vocab::Vocab {
        self.session.vocab()
    }

    pub fn study(&self) -> crate::study::StudyView<'_> {
        crate::study::StudyView::new(self.session.study(), &self.ui.study)
    }

    /// Skills asked, and their answers.
    pub fn skills(&self) -> &mareader_engine::skills::State {
        self.session.skills()
    }

    pub fn page_ai(&self) -> crate::page_ai::PageAiView<'_> {
        crate::page_ai::PageAiView::new(self.session.page_ai(), &self.ui.page_ai)
    }

    /// The pages picked in the thumbnails, in reading order.
    pub fn thumb_picks(&self) -> Vec<usize> {
        self.ui.thumbs.picked()
    }

    /// Tests: the word list window's Markdown, once made.
    pub fn word_window_markdown(&self) -> Option<&str> {
        self.ui.word_window.markdown()
    }

    /// Tests: whether a word list is still being made.
    pub fn making_word_list(&self) -> bool {
        self.session.making_word_list()
    }

    /// Tests: what `pane`'s document keeps of the page AI, oldest first.
    pub fn kept_ai(
        &self,
        pane: PaneId,
    ) -> Vec<(mareader_library::AiTask, mareader_library::PageSpan)> {
        self.session.page_ai_kept(pane)
    }

    /// The marks `pane` shows, wherever they are kept.
    pub fn marks(&self, pane: PaneId) -> Vec<crate::study::marks::Shown> {
        let book = self.book_in(pane);
        let name = self.session.documents().title(pane).unwrap_or_default();
        let look = &self.settings().study.highlights;
        let (state, model) = (self.session.study(), self.session.library_model());
        self.ui
            .study
            .marks_of(state, model, book, name, look)
            .to_vec()
    }

    /// The bar's title: the active pane's document, else the app.
    pub fn title(&self) -> &str {
        self.session
            .documents()
            .title(self.active())
            .unwrap_or("Mareader")
    }
}

impl MareaderApp {
    /// Headless knobs for `examples/shot.rs` and tests.
    ///
    /// The engine parses the keys that set what it holds, reading the session
    /// they arrive at, so a shot, a query string and a command line all meet
    /// one answer. What is left is each family's own: its widgets, its
    /// popovers, and the splits its window has room for.
    pub fn set_option(&mut self, key: &str, value: &str) -> Result<(), String> {
        self.reconcile_texts();
        if let Some(found) = mareader_engine::options::apply(&mut self.session, key, value) {
            for command in found? {
                self.queue(Sent::Engine(command));
            }
            return Ok(());
        }
        if mareader_engine::command_spec::by_id(key).is_some() {
            self.session.execute_id(key, value, 0.0)?;
            self.drain_local();
            return Ok(());
        }
        if key.starts_with("ui.") {
            let cmd = crate::ui_command::parse_ui_command(key)?;
            self.queue(Sent::Ui(cmd));
            return Ok(());
        }
        for family in [
            Family(super::appearance::own_key),
            Family(super::library::own_key),
            Family(super::study::own_key),
            Family(super::page_ai::own_key),
            Family(super::skills::own_key),
            Family(super::study_mode::own_key),
            Family(super::space::own_key),
            Family(super::read_aloud::own_key),
            Family(super::panes::own_key),
        ] {
            if let Some(result) = (family.0)(self, key, value) {
                return result;
            }
        }
        Err(format!("unknown option {key}"))
    }
}

/// One family's own keys, so the door can ask them in turn.
struct Family(fn(&mut MareaderApp, &str, &str) -> Option<Result<(), String>>);
