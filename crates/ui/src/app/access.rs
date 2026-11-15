use super::*;
use crate::app::navigation::found_in;

impl MareaderApp {
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Changes apply next frame, as the settings window's do.
    pub fn settings_mut(&mut self) -> &mut Settings {
        &mut self.settings
    }

    pub fn read_aloud(&self) -> &crate::read_aloud::ReadAloud {
        &self.read_aloud
    }

    /// The file being read: the active pane's, else the first with one.
    pub fn last_document(&self) -> Option<LastDocument> {
        let panes = std::iter::once(self.active).chain(self.tree.panes());
        let (pane, at) =
            (panes.into_iter()).find_map(|pane| Some((pane, self.documents.place(pane)?)))?;
        let place = at.anchor.map(|anchor| (anchor.block, anchor.offset));
        let spine = at.spine.map(|place| mareader_library::SpinePosition {
            href: place.href,
            item: place.item,
            block: place.block,
            offset: place.offset,
        });
        let book = self.documents.book(pane);
        Some(LastDocument {
            path: at.path,
            page: at.page,
            place,
            spine,
            pages: at.pages,
            book,
        })
    }

    pub fn library(&self) -> &mareader_library::Library {
        self.library.model()
    }

    /// The page's own state: the open shelf, a name being edited.
    pub fn library_state(&self) -> &LibraryState {
        &self.library
    }

    /// For tests and tools that shape the library straight on.
    pub fn library_state_mut(&mut self) -> &mut LibraryState {
        &mut self.library
    }

    pub fn library_showing(&self) -> bool {
        self.route() == Route::Library
    }

    /// Covers held as textures, for tests.
    /// Document searches kept, for meters.
    pub fn searches(&self) -> usize {
        self.search.len()
    }

    pub fn covers_shown(&self) -> usize {
        self.library.covers().shown()
    }

    /// The newest toast still up.
    pub fn toast(&self) -> Option<&Toast> {
        self.toasts.current()
    }

    /// Every toast still up, newest first, and whether the stack is open.
    pub fn toasts(&self) -> (Vec<&Toast>, bool) {
        (self.toasts.shown().collect(), self.toasts.is_open())
    }

    /// Applied at the end of the next frame, like a view's commands.
    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Store true here, from any thread, when the system wants less motion.
    pub fn reduced_motion(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.reduced_motion)
    }

    pub fn find(&self) -> &FindBar {
        &self.find
    }

    /// What the find bar's query has found in its pane, if asked yet.
    pub fn found(&self) -> Option<&Found> {
        found_in(&self.search, &self.documents, &self.find)
    }

    pub fn settings_window(&self) -> &SettingsWindow {
        &self.settings_window
    }

    pub fn sidebar(&self) -> &Sidebar {
        &self.sidebar
    }

    /// Where the macOS traffic lights go; the desktop binary moves them.
    pub fn native_lights(&self) -> NativeLights {
        self.lights
    }

    /// Where the binary found the lights after moving them; none when hidden.
    pub fn set_light_spots(&mut self, spots: Option<LightSpots>) {
        self.light_spots = spots;
    }

    pub fn titlebar(&self) -> &TitleBar {
        &self.titlebar
    }

    pub fn panes(&self) -> Vec<PaneId> {
        self.tree.panes()
    }

    /// Each pane's rect as last drawn.
    pub fn pane_rects(&self) -> &[(PaneId, egui::Rect)] {
        &self.pane_rects
    }

    /// Whether a theme change is fading in over the old look.
    pub fn fading(&self) -> bool {
        self.fade.is_fading()
    }

    pub fn active_pane(&self) -> PaneId {
        self.active
    }

    pub fn documents(&self) -> &Documents {
        &self.documents
    }

    /// The study tools: the card, its answer, session marks.
    pub fn dict(&self) -> &crate::dict::Dict {
        &self.dict
    }

    pub fn dict_mut(&mut self) -> &mut crate::dict::Dict {
        &mut self.dict
    }

    /// Whether the active pane's scripts have run and taken every click.
    pub fn scripts_settled(&self) -> bool {
        let pane = self.documents.scripts_idle(self.active);
        self.script_shot.is_empty() && pane && self.scripts.idle()
    }

    /// The scripts' state, for a screenshot that waited too long.
    pub fn scripts_state(&self) -> String {
        let pane = self.documents.scripts_state(self.active);
        format!("{}; {pane}", self.scripts.describe())
    }

    /// The pronunciation pack, for tests and shots.
    pub fn sounds(&self) -> &crate::sounds::Sounds {
        &self.sounds
    }

    /// The word levels: the dataset and the shaded words.
    pub fn vocab(&self) -> &crate::vocab::Vocab {
        &self.vocab
    }

    pub fn study(&self) -> &Study {
        &self.study
    }

    pub fn page_ai(&self) -> &crate::page_ai::PageAi {
        &self.page_ai
    }

    /// The pages picked in the thumbnails, in reading order.
    pub fn thumb_picks(&self) -> Vec<usize> {
        self.thumbs.picked()
    }

    /// Tests: the word list window's Markdown, once made.
    pub fn word_window_markdown(&self) -> Option<&str> {
        self.word_window.markdown()
    }

    /// Tests: whether a word list is still being made.
    pub fn making_word_list(&self) -> bool {
        self.word_lists.busy()
    }

    /// Tests: what `pane`'s document keeps of the page AI, oldest first.
    pub fn kept_ai(
        &self,
        pane: PaneId,
    ) -> Vec<(mareader_library::AiTask, mareader_library::PageSpan)> {
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default();
        self.page_ai.kept(self.library.model(), (book, name))
    }

    /// The marks `pane` shows, wherever they are kept.
    pub fn marks(&self, pane: PaneId) -> Vec<crate::study::marks::Shown> {
        let book = self.library.book_in(&self.documents, pane);
        let name = self.documents.title(pane).unwrap_or_default();
        let look = &self.settings.study.highlights;
        self.study
            .marks_of(self.library.model(), book, name, look)
            .to_vec()
    }

    /// The bar's title: the active pane's document, else the app.
    pub fn title(&self) -> &str {
        self.documents.title(self.active).unwrap_or("Mareader")
    }
}
