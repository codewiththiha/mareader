use super::*;

impl MareaderApp {
    /// The page in the window; the library replaces the panes, not ends them.
    pub fn route(&self) -> Route {
        match (self.dict_from, self.library.showing) {
            (Some(_), _) => Route::Dictionary,
            (None, true) => Route::Library,
            (None, false) => Route::Reader,
        }
    }

    /// Something is open in a pane, so the library can go back to it.
    pub(super) fn reading(&self) -> bool {
        let panes = self.tree.panes();
        panes.iter().any(|p| self.documents.reading(*p).is_some())
    }

    /// The one way between routes; leaving the reader ends its overlays.
    pub(super) fn go(&mut self, route: Route) {
        if route == Route::Library {
            self.settings_window.close();
        }
        if route != Route::Reader {
            self.find.close();
        }
        let here = self.route();
        self.dict_from = match route {
            Route::Dictionary if here == Route::Dictionary => self.dict_from,
            Route::Dictionary => Some(here),
            _ => None,
        };
        if route == Route::Dictionary {
            self.dict.arrive();
        } else {
            self.library.showing = route == Route::Library;
        }
    }

    /// The floating Dictionary: its state lives in the settings.
    pub(super) fn dict_window(&mut self, w: crate::dict::Window) {
        use crate::dict::Window as W;
        if !matches!(w, W::Place(_)) {
            self.read_aloud.yield_to_dictionary();
        }
        let win = &mut self.settings.dictionary.window;
        match w {
            W::Toggle => {
                let show = !win.open || win.minimized;
                (win.open, win.minimized) = (show, false);
            }
            W::Focus => (win.open, win.minimized) = (true, false),
            W::Close => win.open = false,
            W::Minimize(folded) => win.minimized = folded,
            W::Place(r) => win.rect = Some([r.min.x, r.min.y, r.width(), r.height()]),
            W::Expand => {
                win.open = false;
                self.go(Route::Dictionary);
                return;
            }
        }
        // Shown, its field takes the keys.
        if win.open && !win.minimized && !matches!(w, W::Place(_)) {
            self.dict.arrive();
        }
    }

    /// The card's word, searched where the Dictionary opens from here.
    pub(super) fn pursue(&mut self, word: &str) {
        self.dict.escape();
        let floats = self.settings.dictionary.opens == DictionaryOpens::Window;
        if self.route() == Route::Reader && floats {
            self.dict_window(crate::dict::Window::Focus);
        } else {
            self.go(Route::Dictionary);
        }
        self.dict.search(word);
    }

    /// Where the Dictionary page's pressed button returns.
    pub(super) fn dict_back(&self) -> Route {
        match self.dict_from {
            Some(Route::Reader) if !self.reading() => Route::Library,
            Some(Route::Reader) => Route::Reader,
            _ => Route::Library,
        }
    }

    /// Tabbing into a pane makes it the focused one, as a click there does.
    pub(super) fn follow_focus(&self, ctx: &Context, out: &mut Commands) {
        let Some(at) = crate::focus::rect(ctx).map(|r| r.center()) else {
            return;
        };
        let modal =
            self.settings_window.is_open() || self.word_window.is_open() || self.library.sheet_up();
        let pane = self.pane_rects.iter().find(|(_, r)| r.contains(at));
        if let Some((id, _)) = pane.filter(|(id, _)| !modal && *id != self.active) {
            out.push(Command::FocusPane(*id));
        }
    }

    /// Asks for the bar's query once it settles, and walks to its hits.
    pub(super) fn drive_search(&mut self, ctx: &Context) {
        let Some(pane) = self.find.pane() else {
            return;
        };
        let Some(reading) = self.documents.reading(pane) else {
            return;
        };
        if !reading.document.is_complete() {
            return;
        }
        let key = Arc::as_ptr(&reading.document).addr();
        if self.find.due(ctx.input(|i| i.time), key) {
            let query = self.find.query();
            self.search.ask(ctx, reading.doc, &reading.document, query);
            self.find.asked(key);
        }
        let found = self.search.found(reading.doc, &reading.document);
        let found = found.filter(|f| f.query == self.find.query());
        self.find.settle(found, reading.current);
        let Some(i) = self.find.seeking() else {
            return;
        };
        ctx.request_repaint();
        let Some(hit) = found.and_then(|f| f.hits.get(i)) else {
            self.find.sought();
            return;
        };
        if self.documents.reveal_hit(pane, hit.page, hit.chars.clone()) {
            self.find.sought();
        }
    }

    /// Opens settings on `tab`; away from the books, only their tabs.
    pub(super) fn open_settings(&mut self, tab: Option<Tab>) {
        let route = self.route();
        let limited = route != Route::Reader && tab.is_none_or(Tab::beyond_books);
        let tab = match (route, tab) {
            (Route::Dictionary, None) => Some(Tab::Dictionary),
            _ => tab,
        };
        self.settings_window.open(tab, limited);
    }

    /// Puts the focused pane first in `recent`, dropping closed panes.
    pub(super) fn note_focus(&mut self) {
        let placed = self.tree.panes();
        self.recent
            .retain(|p| placed.contains(p) && *p != self.active);
        self.recent.insert(0, self.active);
        let unseen: Vec<PaneId> = placed
            .into_iter()
            .filter(|p| !self.recent.contains(p))
            .collect();
        self.recent.extend(unseen);
    }
}

/// What the find bar's pane holds for its current query, if asked yet.
pub(super) fn found_in<'a>(
    search: &'a Search,
    documents: &Documents,
    find: &FindBar,
) -> Option<&'a Found> {
    let reading = documents.reading(find.pane()?)?;
    let found = search.found(reading.doc, &reading.document)?;
    (found.query == find.query()).then_some(found)
}
