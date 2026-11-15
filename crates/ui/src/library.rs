//! The library's state: the model, its files, and what the panes read.

mod actions;
mod add;
pub mod asks;
pub mod books;
mod copying;
mod counted;
mod covers;
pub mod crumbs;
mod duplicating;
mod find;
pub mod folders;
mod import_sheet;
mod io;
mod living;
pub(crate) mod menu;
mod menus;
pub mod moving;
pub mod organizing;
pub mod picks;
mod remove_alert;
mod review;
pub mod runs;
pub mod search;
pub mod sheets;
pub mod shelving;
pub mod view;

use std::collections::{BTreeMap, BTreeSet};
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};

use egui::Context;
use mareader_library::{
    BookId, Copied, DeviceId, Format, Found, Library, Opened, Position, ShelfId, SpinePosition,
    disk::Folder, display_title,
};
use mareader_session::{Anchor, Document, Source, SpinePlace};
use mareader_workspace::PaneId;

use crate::command::{Command, Commands};
use crate::documents::{Documents, OpenRequest};
use crate::toast::Toast;
pub use actions::{Action, Named, Scope, Sheet};
pub use add::ADD_MENU;
pub use books::BookAction;
pub use covers::Covers;
pub use find::{Finding, Step};
use io::{Io, Job, News, Probe};
pub use menu::LIBRARY_VIEW_MENU;
pub use menus::{Organizer, organize_rows};
pub use picks::{Pick, PickAction, Picks};
pub use search::{Field, Listing, SearchAction};
pub use shelving::{Carry, Crumb, ShelfAction};

/// Seconds between writes while reading.
pub const SAVE_DELAY: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    /// Its files are being read; opens wait for their positions.
    Loading,
    Ready,
    /// A newer app's files, or none readable: kept in memory only.
    ReadOnly,
}

pub struct LibraryState {
    model: Library,
    /// None: in memory only, as in tests and shots.
    folder: Option<Folder>,
    status: Status,
    io: Io,
    inline: bool,
    /// The file each pane showed when last looked at, and as which book.
    seen: BTreeMap<PaneId, (PathBuf, Option<BookId>)>,
    /// Opens held until the library has loaded.
    waiting: Vec<OpenRequest>,
    /// When the last write left, in egui seconds.
    wrote: f64,
    covers: Covers,
    folders: folders::Folders,
    /// The library page is up in place of the panes.
    pub showing: bool,
    pub scope: Scope,
    /// The shelf open on the page; none is the top level.
    at: Option<ShelfId>,
    /// The shelf whose name the breadcrumb is editing in place.
    renaming: Option<ShelfId>,
    /// A shelf the rail's tree just made, for it to name.
    made: Option<ShelfId>,
    picks: Picks,
    /// Shelf rows the list shows unfolded; going elsewhere folds them.
    unfolded: BTreeSet<ShelfId>,
    /// The shelf under files dragged in from outside, if any.
    pub(crate) file_target: Option<ShelfId>,
    search: search::Search,
    /// Imports and folder walks still going.
    pub runs: runs::Runs,
    motion: living::Motion,
    /// The AI's run filing loose books, and its plan.
    organizer: organizing::Organizer,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            model: Library::new(new_device()),
            folder: None,
            status: Status::Ready,
            io: Io::default(),
            inline: false,
            seen: BTreeMap::new(),
            waiting: Vec::new(),
            wrote: f64::NEG_INFINITY,
            covers: Covers::default(),
            folders: folders::Folders::default(),
            showing: false,
            scope: Scope::All,
            at: None,
            renaming: None,
            made: None,
            picks: Picks::default(),
            unfolded: BTreeSet::new(),
            file_target: None,
            search: search::Search::default(),
            runs: runs::Runs::default(),
            motion: living::Motion::default(),
            organizer: organizing::Organizer::default(),
        }
    }
}

impl LibraryState {
    pub fn model(&self) -> &Library {
        &self.model
    }

    /// Paints `book`'s cover in `rect`; false when it isn't shelved.
    pub fn paint_cover(&mut self, ui: &egui::Ui, rect: egui::Rect, book: BookId) -> bool {
        let Some(found) = self.model.book(book) else {
            return false;
        };
        let t = crate::theme::Tokens::get(ui.ctx());
        view::thumb(ui, rect, found, &mut self.covers, &t);
        true
    }

    /// For edits outside the library's own: marks, from the study tools.
    pub fn model_mut(&mut self) -> &mut Library {
        &mut self.model
    }

    pub fn loading(&self) -> bool {
        self.status == Status::Loading
    }

    /// Disk work runs on the frame; set before [`Self::attach`].
    pub fn set_inline(&mut self, inline: bool) {
        self.inline = inline;
        self.covers.set_inline(inline);
    }

    /// Keeps covers in `<dir>/covers` between launches.
    pub fn attach_cache(&mut self, dir: PathBuf) {
        self.covers.set_cache(dir.join("covers"));
    }

    /// The books, and their covers to draw them with.
    pub fn parts(&mut self) -> (&Library, &mut Covers) {
        (&self.model, &mut self.covers)
    }

    /// What the page draws with, its grid's moving parts included.
    fn page_parts(&mut self) -> (&Library, &mut Covers, &mut living::Motion) {
        (&self.model, &mut self.covers, &mut self.motion)
    }

    /// The books and shelves picked on the page.
    pub fn picks(&self) -> &Picks {
        &self.picks
    }

    pub fn unfolded(&self) -> &BTreeSet<ShelfId> {
        &self.unfolded
    }

    /// Escape lets go of a selection, then the search; true if it did.
    pub fn escape(&mut self) -> bool {
        if self.picks.active() {
            self.picks.apply(PickAction::Clear);
            return true;
        }
        self.search.clear()
    }

    pub fn covers(&self) -> &Covers {
        &self.covers
    }

    /// Hands the frame's cover asks to their worker.
    pub fn end_frame(&mut self, ctx: &Context) {
        self.covers.end_frame(ctx);
    }

    /// Reads the library kept in `root`, and saves to it from now on.
    pub fn attach(&mut self, root: PathBuf) {
        let folder = Folder::new(root);
        self.io = if self.inline {
            Io::default()
        } else {
            Io::thread()
        };
        self.io.send(Job::Load(folder.clone(), self.model.device()));
        self.folder = Some(folder);
        self.status = Status::Loading;
    }

    /// Takes in finished disk work.
    pub fn receive(&mut self, ctx: &Context, out: &mut Commands) {
        let now = ctx.input(|i| i.time);
        for news in self.io.poll(ctx) {
            match news {
                News::Loaded(loaded) => {
                    // Opens before loading can't have touched the model.
                    let loaded = *loaded;
                    self.model = loaded.library;
                    self.status = match loaded.writable {
                        true => Status::Ready,
                        false => Status::ReadOnly,
                    };
                    let notes = loaded.notes.into_iter().map(Toast::error);
                    out.extend(notes.map(Command::Notify));
                    let held = self.waiting.drain(..);
                    out.extend(held.map(Command::Open));
                    // Gone files and new books, found where they're kept.
                    self.rescan(now);
                }
                News::Measured(probe, measure) => {
                    let opened = Opened {
                        path: probe.path,
                        measure,
                        format: probe.format,
                        title: probe.title,
                        author: probe.author,
                    };
                    self.model.record_open_as(opened, wall_ms(), probe.book);
                }
                News::WriteFailed(e) => out.push(Command::Notify(Toast::error(e))),
                News::Walked(done) => self.walked(*done, now, out),
                News::Files(done) => self.measured(*done, now, out),
                News::Copied(done) => self.copied(*done, now, out),
                News::Checked(gone) => self.checked(&gone),
                News::Relinked(relink, measured) => self.relink_measured(relink, measured, out),
                News::Sought {
                    book,
                    folder,
                    found,
                } => self.sought_found(book, &folder, found, out),
                News::Beat(beat) => self.runs.beat(beat),
            }
        }
    }

    /// Fills in where an open file was left; held while loading.
    pub fn resume(&mut self, mut request: OpenRequest) -> Option<OpenRequest> {
        let Source::Path(path) = &request.source else {
            return Some(request);
        };
        // Opened as a book, or with a place already: nothing to fill in.
        if request.page != 0
            || request.place.is_some()
            || request.spine.is_some()
            || request.pages > 0
            || request.book.is_some()
        {
            return Some(request);
        }
        if self.status == Status::Loading {
            self.waiting.push(request);
            return None;
        }
        // Recorded as `Documents::open` spells it: canonical.
        let real = std::fs::canonicalize(path).unwrap_or_else(|_| path.clone());
        let at = self.model.book_at(&real);
        if let Some(p) = at.and_then(|id| self.model.position(id)) {
            request.page = p.page;
            request.place = p.place.map(|(block, offset)| Anchor { block, offset });
            request.spine = p.spine.map(|place| SpinePlace {
                href: place.href,
                item: place.item,
                block: place.block,
                offset: place.offset,
            });
            request.pages = p.pages;
        }
        Some(request)
    }

    /// Records opens and positions; true if a file went to be measured.
    pub fn observe(&mut self, documents: &Documents, panes: &[PaneId]) -> bool {
        let mut measuring = false;
        if self.status == Status::Loading {
            return measuring;
        }
        let now = wall_ms();
        for &pane in panes {
            let (Some(reading), Some(place)) = (documents.reading(pane), documents.place(pane))
            else {
                self.seen.remove(&pane);
                continue;
            };
            // The view carries its book: a failed open leaves nothing.
            let reader = documents.book(pane);
            let shown = (place.path, reader);
            if self.seen.get(&pane) != Some(&shown) {
                let (path, book) = shown.clone();
                self.seen.insert(pane, shown);
                self.io.send(Job::Measure(Box::new(Probe {
                    format: format_of(&path, &reading.document),
                    title: title_of(&reading.document, &path),
                    author: author_of(&reading.document),
                    path,
                    book,
                })));
                measuring = true;
                continue;
            }
            let (path, reader) = shown;
            // A book removed while read keeps no place, nor hands it on.
            let id = match reader {
                Some(id) => self.model.book(id).map(|b| b.id),
                None => self.model.book_at(&path),
            };
            let Some(id) = id else {
                continue;
            };
            let position = Position {
                page: place.page,
                pages: place.pages,
                place: place
                    .anchor
                    .filter(|_| reading.document.is_complete())
                    .map(|anchor| (anchor.block, anchor.offset)),
                spine: place.spine.map(|spine| SpinePosition {
                    href: spine.href,
                    item: spine.item,
                    block: spine.block,
                    offset: spine.offset,
                }),
            };
            self.model.record_position(id, position, now);
        }
        self.seen.retain(|p, _| panes.contains(p));
        measuring
    }

    /// The book `pane` reads as: its own, else its file's.
    pub fn book_in(&self, documents: &Documents, pane: PaneId) -> Option<BookId> {
        let own = documents.book(pane);
        own.or_else(|| self.model.book_at(&documents.place(pane)?.path))
    }

    /// Writes changes at most every [`SAVE_DELAY`]; else the wait left.
    pub fn save(&mut self, now: f64) -> Option<f64> {
        let (Some(_), Status::Ready) = (&self.folder, self.status) else {
            return None;
        };
        if !self.model.is_dirty() {
            return None;
        }
        let wait = self.wrote + SAVE_DELAY - now;
        if wait > 0.0 {
            return Some(wait);
        }
        let dirty = self.model.take_dirty();
        let changes = self.model.changes(&dirty);
        self.io.send(Job::Write(Box::new(changes)));
        self.wrote = now;
        None
    }

    /// Sends every pending change now: the app is saving or exiting.
    pub fn flush(&mut self) {
        self.wrote = f64::NEG_INFINITY;
        self.save(0.0);
    }

    /// Where `id`'s file opens, on its page; a toast if it's gone.
    pub fn open(&mut self, id: BookId) -> Result<OpenRequest, Toast> {
        let book = self
            .model
            .book(id)
            .ok_or_else(|| Toast::error("That book was removed."))?;
        let Some(path) = book.path() else {
            let title = &book.title.value;
            return Err(Toast::error(format!("“{title}” isn't where it was.")));
        };
        let position = self.model.position(id);
        let (page, place, spine, pages) = match position {
            Some(position) => (
                position.page,
                position
                    .place
                    .map(|(block, offset)| Anchor { block, offset }),
                position.spine.map(|place| SpinePlace {
                    href: place.href,
                    item: place.item,
                    block: place.block,
                    offset: place.offset,
                }),
                position.pages,
            ),
            None => (0, None, None, 0),
        };
        // Read as this book: a twin keeps its own place.
        Ok(OpenRequest {
            book: Some(id),
            page,
            place,
            spine,
            pages,
            ..OpenRequest::path(path.to_path_buf())
        })
    }

    /// Adds made-up imported copies, on no shelf, for shots and tests.
    pub fn seed_copies(&mut self, count: usize) {
        let from = self.model.len();
        let pairs: Vec<(Found, Copied)> = (from..from + count)
            .map(|i| {
                let (title, author) = SEEDS
                    .get(i % SEEDS.len())
                    .copied()
                    .unwrap_or(("Book", None));
                let measure = mareader_library::Measure {
                    content: mareader_library::ContentId(i as u128 + 1),
                    stamp: Default::default(),
                };
                let path = PathBuf::from(format!("/store/{i}-{title}.epub"));
                let found = Found {
                    path: path.clone(),
                    format: Format::Epub,
                    measure,
                    title: Some(title.to_owned()),
                    author: author.map(str::to_owned),
                };
                (found, Copied { path, measure })
            })
            .collect();
        self.model.land_copies(&pairs, None, wall_ms());
    }

    /// Adds made-up books for shots: some started, one missing.
    pub fn seed(&mut self, count: usize) {
        let formats = [Format::Epub, Format::Pdf, Format::Markdown, Format::Text];
        let now = wall_ms();
        let from = self.model.len();
        for i in from..from + count {
            let (title, author) = SEEDS
                .get(i % SEEDS.len())
                .copied()
                .unwrap_or(("Book", None));
            let format = formats
                .get(i % formats.len())
                .copied()
                .unwrap_or(Format::Pdf);
            let opened = Opened {
                path: PathBuf::from(format!("/books/{i}-{title}")),
                measure: mareader_library::Measure {
                    content: mareader_library::ContentId(i as u128 + 1),
                    stamp: Default::default(),
                },
                format,
                title: title.to_owned(),
                author: author.map(str::to_owned),
            };
            let at = now.saturating_sub(i as u64 * 7_000_000);
            let id = self.model.record_open(opened, at);
            if i % 3 != 2 {
                let pages = 40 + 37 * i;
                let position = Position {
                    page: pages * (i - from + 1) / (count + 2),
                    pages,
                    place: None,
                    spine: None,
                };
                self.model.record_position(id, position, at);
            }
        }
        // The fourth seeded book has lost its file.
        self.model
            .check_present(|p| !p.to_string_lossy().starts_with("/books/3-"));
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

/// Titles the made-up books wear.
const SEEDS: [(&str, Option<&str>); 10] = [
    ("The Raven", Some("Edgar Allan Poe")),
    ("Middlemarch", Some("George Eliot")),
    ("A Field Guide to Ferns", None),
    ("Notes on Typography", Some("Ada Bringhurst")),
    ("The Voyage Out", Some("Virginia Woolf")),
    ("Collected Essays, Volume 10", Some("M. Aurel")),
    ("Letters from the Coast", None),
    ("An Atlas of Rivers", Some("Lena Ortiz")),
    ("Collected Essays, Volume 2", Some("M. Aurel")),
    ("Practical Rust", Some("Sam Okafor")),
];

/// Random per library: the first save keeps it in the store.
fn new_device() -> DeviceId {
    DeviceId(RandomState::new().hash_one(wall_ms()))
}

/// Milliseconds since 1970; the browser preview has no clock here.
pub fn wall_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        since.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
    #[cfg(target_arch = "wasm32")]
    0
}
