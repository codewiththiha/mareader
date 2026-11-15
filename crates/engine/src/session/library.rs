//! Library state and position tracking held by [`Session`].

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mareader_library::{
    BookId, Format, Library, Position, ShelfId, SpinePosition, Suggestion, View as LibraryView,
    display_title,
};
use mareader_session::{Anchor, Document};
use mareader_workspace::PaneId;

use crate::documents::OpenRequest;
use crate::event::Event;
use crate::library::{
    Action, Heard, Job, Listing, Lit, Out, Paused, Probe, State, Status, wall_ms,
};
use crate::reading::Place;
use crate::toast::Toast;

use super::Session;

impl Session {
    pub fn library(&self) -> &State {
        &self.library
    }

    pub fn library_model(&self) -> &Library {
        self.library.model()
    }

    /// Creates a shelf directly on the library model, for test fixtures.
    pub fn library_seed_shelf(
        &mut self,
        name: &str,
    ) -> Result<ShelfId, mareader_library::ShelfError> {
        self.library.model_mut().create_shelf(name, None, 1)
    }

    /// Creates and names a link at the open shelf, for test fixtures.
    pub fn library_seed_link(
        &mut self,
        target: mareader_library::LinkTo,
        name: &str,
    ) -> Option<mareader_library::LinkId> {
        let at = self.library.at();
        let id = self.library.model_mut().link_to(target, at, 1)?;
        self.library.model_mut().rename_link(id, name, 2);
        self.library.settle();
        Some(id)
    }

    /// Attaches the library folder using the session's waker.
    pub fn wire_library(&mut self, root: PathBuf) {
        self.library.attach(root, Arc::clone(&self.waker));
    }

    /// Keeps covers in `<dir>/covers` between launches.
    pub fn wire_library_cache(&mut self, dir: PathBuf) {
        self.library.attach_cache(dir);
    }

    /// Takes in finished library disk work; returns emitted toasts/commands.
    pub(crate) fn library_receive(&mut self, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        self.library.receive(now, &mut out);
        out
    }

    /// Rescans when the window returns, and after files settle.
    pub fn library_tick(&mut self, now: f64, focused: bool) -> Option<f64> {
        self.library.tick(now, focused)
    }

    /// Writes pending library changes when due; returns wait left if skipped.
    pub(crate) fn library_save(&mut self, now: f64) -> Option<f64> {
        self.library.save(now)
    }

    /// Records opens and positions; true if a file went to be measured.
    pub fn library_observe(
        &mut self,
        view_anchor: impl Fn(PaneId, &Place) -> Option<Anchor>,
    ) -> bool {
        let mut measuring = false;
        if self.library.status == Status::Loading {
            return measuring;
        }
        let panes = self.tree.panes();
        let now = wall_ms();
        for &pane in &panes {
            let Some(document) = self.documents.document_of(pane) else {
                self.library.seen.remove(&pane);
                continue;
            };
            let Some(place) = self.documents.resume_place(pane, |p| view_anchor(pane, p)) else {
                self.library.seen.remove(&pane);
                continue;
            };
            let book = self.documents.book(pane);
            let key = (place.path.clone(), book);
            if self.library.seen.get(&pane) != Some(&key) {
                let (path, book) = key.clone();
                self.library.seen.insert(pane, key);
                self.library.io.send(Job::Measure(Box::new(Probe {
                    format: format_of(&path, &document),
                    title: title_of(&document, &path),
                    author: author_of(&document),
                    path,
                    book,
                })));
                measuring = true;
                continue;
            }
            let (path, reader) = key;
            let id = match reader {
                Some(id) => self.library.model.book(id).map(|b| b.id),
                None => self.library.model.book_at(&path),
            };
            let Some(id) = id else {
                continue;
            };
            let position = Position {
                page: place.page,
                pages: place.pages,
                place: place
                    .anchor
                    .filter(|_| document.is_complete())
                    .map(|anchor| (anchor.block, anchor.offset)),
                spine: place.spine.map(|spine| SpinePosition {
                    href: spine.href,
                    item: spine.item,
                    block: spine.block,
                    offset: spine.offset,
                }),
            };
            self.library.model.record_position(id, position, now);
        }
        self.library.seen.retain(|p, _| panes.contains(p));
        measuring
    }

    /// Applies a library action; returns resulting toasts and commands.
    pub(crate) fn library_act(&mut self, action: Action, now: f64) -> Vec<Out> {
        let mut out = Vec::new();
        self.library.act(action, now, &mut out);
        out
    }

    /// Feeds an AI organizer update into the library; returns toasts/commands.
    pub fn library_heard(&mut self, heard: Heard) -> Vec<Out> {
        let mut out = Vec::new();
        self.library.heard(heard, &mut out);
        out
    }

    pub(crate) fn library_reveal_book(&mut self, book: BookId) {
        let path = self.library.model().book(book).and_then(|b| b.path());
        if !path.is_some_and(|p| self.host.reveal(p)) {
            self.emit(Event::Toast(Toast::error(
                "The file manager couldn't be opened.",
            )));
        }
    }

    pub(crate) fn library_reveal_shelf(&mut self, shelf: ShelfId) {
        let dir = self.library.model().shelf_dir(shelf);
        if !dir.as_deref().is_some_and(|d| self.host.reveal(d)) {
            self.emit(Event::Toast(Toast::error(
                "The file manager couldn't be opened.",
            )));
        }
    }

    pub(crate) fn library_resume(&mut self, request: OpenRequest) -> Option<OpenRequest> {
        self.library.resume(request)
    }

    pub(crate) fn library_open(&mut self, id: BookId) -> Result<OpenRequest, Toast> {
        self.library.open(id)
    }

    pub fn library_settle(&mut self) {
        self.library.settle();
    }

    pub fn library_listing(&mut self, view: &LibraryView) -> Rc<Listing> {
        self.library.listing(view)
    }

    pub fn library_revealing(&mut self, now: f64) -> Option<(Lit, bool)> {
        self.library.revealing(now)
    }

    pub fn library_suggestions(&mut self) -> Rc<[Suggestion]> {
        self.library.suggestions()
    }

    pub fn library_take_made(&mut self) -> Option<ShelfId> {
        self.library.take_made()
    }

    /// Advances visible run display delays; returns `(due_flags, waiting)`.
    pub fn library_runs_due(&mut self, now: f64, show_paused: bool) -> (Vec<bool>, bool) {
        let mut due = Vec::with_capacity(self.library.runs.list().len());
        let mut waiting = false;
        for run in self.library.runs.list_mut() {
            if run.paused() && !show_paused {
                due.push(false);
                continue;
            }
            let ready = run.due(now);
            if !ready {
                waiting = true;
            }
            due.push(ready);
        }
        (due, waiting)
    }

    pub fn library_set_paused(&mut self, paused: Option<Paused>) {
        self.library.set_paused(paused);
    }

    pub fn library_organizer_end(&mut self) {
        self.library.organizer_end();
    }

    pub fn library_close_replace(&mut self) {
        self.library.close_replace();
    }

    pub fn library_ask_replace(&mut self, picked: Option<Vec<BookId>>) {
        self.library.ask_replace(picked);
    }

    pub fn library_organizing(&mut self, books: impl IntoIterator<Item = BookId>, label: &str) {
        self.library.organizing(books, label);
    }
}

/// The file's own format; the document says if it's a book.
fn format_of(path: &Path, document: &Document) -> Format {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    match mareader_epub::Format::of_name(name) {
        Some(mareader_epub::Format::Plain) => Format::Text,
        Some(mareader_epub::Format::Markdown) => Format::Markdown,
        None if document.is_reflowable() => Format::Epub,
        None => Format::Pdf,
    }
}

/// A book's own title, unless generic; else the file's name.
fn title_of(document: &Document, path: &Path) -> String {
    let own = match document {
        Document::Epub { book, .. } => book.title(),
        Document::Pdf(pdf) => pdf.raw_title(),
    };
    display_title(own, path)
}

/// The document's own author, if it names one.
fn author_of(document: &Document) -> Option<String> {
    let own = match document {
        Document::Epub { book, .. } => book.author(),
        Document::Pdf(pdf) => pdf.raw_author(),
    };
    own.map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_owned)
}
