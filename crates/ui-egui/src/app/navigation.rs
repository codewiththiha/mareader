use egui::Context;

use super::*;

/// Seconds words wait for their book to open before they are dropped.
const FIND_ON_OPEN_SECS: f64 = 15.0;

impl MareaderApp {
    /// The page in the window; the library replaces the panes, not ends them.
    pub fn route(&self) -> Route {
        self.session.route()
    }

    /// Something is open in a pane, so the library can go back to it.
    pub(super) fn reading(&self) -> bool {
        let panes = self.tree().panes();
        let docs = self.session.documents();
        panes.iter().any(|p| docs.place(*p).is_some())
    }

    /// The one way between routes; leaving the reader ends its overlays.
    pub(super) fn go(&mut self, route: Route) {
        self.send(Command::ShowRoute(route));
    }

    /// What the page left behind had open closes; the new one settles in.
    pub(super) fn arrived(&mut self, route: Route) {
        if route == Route::Library {
            self.ui.settings_window.close();
        }
        if route != Route::Reader {
            self.ui.find.close();
        }
        if route == Route::Dictionary {
            self.ui.dict.arrive();
        }
    }

    /// The floating Dictionary: its state lives in the settings.
    pub(super) fn dict_window(&mut self, w: mareader_engine::dict::Window) {
        use mareader_engine::dict::Window as W;
        if !matches!(w, W::Place(_)) {
            self.ui.read_aloud.yield_to_dictionary();
        }
        let mut win = self.settings().dictionary.window.clone();
        match w {
            W::Toggle => {
                let show = !win.open || win.minimized;
                (win.open, win.minimized) = (show, false);
            }
            W::Focus => (win.open, win.minimized) = (true, false),
            W::Close => win.open = false,
            W::Minimize(folded) => win.minimized = folded,
            W::Place(r) => win.rect = Some(r),
            W::Expand => {
                win.open = false;
                let _ = self.set_field("dictionary.window", win.clone());
                self.go(Route::Dictionary);
                return;
            }
        }
        let _ = self.set_field("dictionary.window", win.clone());
        // Shown, its field takes the keys.
        if win.open && !win.minimized && !matches!(w, W::Place(_)) {
            self.ui.dict.arrive();
        }
    }

    /// The card's word, searched where the Dictionary opens from here.
    pub(super) fn pursue(&mut self, word: &str) {
        self.ui.dict.escape();
        self.ui.study.escape(&mut self.session);
        let floats = self.settings().dictionary.opens == DictionaryOpens::Window;
        if self.route() == Route::Reader && floats {
            self.dict_window(mareader_engine::dict::Window::Focus);
        } else {
            self.go(Route::Dictionary);
        }
        self.ui.dict.search(word);
    }

    /// The Dictionary page's pressed button, named for where it returns.
    pub(super) fn dict_back_label(&self) -> &'static str {
        match self.dict_back() {
            Route::Reader => "Back to reading",
            Route::Space => "Back to Space",
            _ => "Back to the library",
        }
    }

    /// Space's title in the bar: the hub's, or the tool's it shows.
    pub(super) fn space_title(&self) -> &'static str {
        self.session.room().title()
    }

    /// Where the Dictionary page's pressed button returns.
    pub(super) fn dict_back(&self) -> Route {
        match self.session.dict_from() {
            Some(Route::Reader) if !self.reading() => Route::Library,
            Some(Route::Reader) => Route::Reader,
            Some(Route::Space) => Route::Space,
            _ => Route::Library,
        }
    }

    /// Tabbing into a pane makes it the focused one, as a click there does.
    pub(super) fn follow_focus(&self, ctx: &Context, out: &mut Commands) {
        let Some(at) = crate::focus::rect(ctx).map(|r| r.center()) else {
            return;
        };
        let modal = self.ui.settings_window.is_open()
            || self.ui.word_window.is_open()
            || self.session.library().sheet_up();
        let pane = self.ui.pane_rects.iter().find(|(_, r)| r.contains(at));
        if let Some((id, _)) = pane.filter(|(id, _)| !modal && *id != self.active()) {
            out.push(Sent::Engine(Command::FocusPane(*id)));
        }
    }

    /// Opens the find bar on words asked for once their book is read.
    fn find_on_open(&mut self, ctx: &Context) {
        let pending = self.ui.find_on_open.as_ref();
        let Some((book, asked)) = pending.map(|(book, _, at)| (*book, *at)) else {
            return;
        };
        if ctx.input(|i| i.time) - asked > FIND_ON_OPEN_SECS {
            self.ui.find_on_open = None;
            return;
        }
        let pane = self.active();
        let reading = self.documents().reading(pane);
        let ready = reading.is_some_and(|r| r.document.is_complete());
        if !ready || self.session.book_in(pane) != Some(book) {
            ctx.request_repaint();
            return;
        }
        if let Some((_, quote, _)) = self.ui.find_on_open.take() {
            self.ui.find.open(pane);
            self.ui.find.seed(&quote);
            self.ui.titlebar.reveal();
        }
    }

    /// Asks for the bar's query once it settles, and walks to its hits.
    pub(super) fn drive_search(&mut self, ctx: &Context) {
        self.find_on_open(ctx);
        let Some(pane) = self.ui.find.pane() else {
            return;
        };
        let Some(reading) = self.documents().reading(pane) else {
            return;
        };
        if !reading.document.is_complete() {
            return;
        }
        let key = Arc::as_ptr(&reading.document).addr();
        if self.ui.find.due(ctx.input(|i| i.time), key) {
            let query = self.ui.find.query().to_owned();
            let now = ctx.input(|i| i.time);
            self.session.execute(Command::Search { pane, query, case_sensitive: self.ui.find.case_sensitive() }, now);
            self.ui.find.asked(key);
        }
        let found = self.session.search().found(reading.doc, &reading.document);
        let found = found.filter(|f| f.query == self.ui.find.query() && f.case_sensitive == self.ui.find.case_sensitive());
        self.ui.find.settle(found, reading.current);
        let Some(i) = self.ui.find.seeking() else {
            return;
        };
        ctx.request_repaint();
        let Some(hit) = found.and_then(|f| f.hits.get(i)) else {
            self.ui.find.sought();
            return;
        };
        let (page, chars) = (hit.page, hit.chars.clone());
        if self
            .ui
            .documents
            .reveal_hit(&mut self.session, pane, page, chars)
        {
            self.ui.find.sought();
        }
    }

    /// Opens settings on `tab`; away from the books, only their tabs.
    pub(super) fn open_settings(&mut self, tab: Option<Tab>) {
        self.ui.find.close();
        let route = self.route();
        let limited = route != Route::Reader && tab.is_none_or(Tab::beyond_books);
        let tab = match (route, tab) {
            (Route::Dictionary, None) => Some(Tab::Dictionary),
            _ => tab,
        };
        self.ui.settings_window.open(tab, limited);
    }

    /// Puts the focused pane first in `recent`, dropping closed panes.
    ///
    /// The recency list is the engine's, so this goes through its door.
    pub(super) fn note_focus(&mut self) {
        self.send(Command::FocusPane(self.active()));
    }
}

/// What the find bar's pane holds for its current query, if asked yet.
pub(super) fn found_in<'a>(
    search: &'a Search,
    documents: DocumentsView<'_>,
    find: &FindBar,
) -> Option<&'a Found> {
    let reading = documents.reading(find.pane()?)?;
    let found = search.found(reading.doc, &reading.document)?;
    (found.query == find.query() && found.case_sensitive == find.case_sensitive()).then_some(found)
}
