//! The open dialog and dropped files; results arrive later as commands.
//!
//! A dialog belongs to the machine, so a front end only asks for one:
//! [`Dialogs`] is answered by `apps/mareader` over `rfd` on a helper thread, by
//! [`Browser`] where a page has no paths to show, and by [`Quiet`] in a test,
//! which refuses, so the toast that says so is what a reader gets. Law 6 keeps
//! the engine out of all three: what comes back is a command with a path in it,
//! and what crosses to the shell is a `Waker`, never a window.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use egui::{Context, DroppedFileHandle};
use mareader_engine::command::Command;
use mareader_engine::documents::{OpenRequest, Placement};
use mareader_engine::export::Format;
use mareader_engine::session::Waker;
use mareader_engine::toast::Toast;
use mareader_workspace::PaneId;

use crate::repaint::wake;
use crate::ui_command::{Commands, Sent, UiCommand};

/// What the open dialog admits.
/// Markdown and plain text open converted to books.
const BOOKS: [&str; 7] = ["pdf", "epub", "txt", "text", "md", "markdown", "mdown"];

/// One filter a dialog shows: its name, and the extensions it admits.
pub type Filter = (&'static str, &'static [&'static str]);

/// What a front end asks a dialog for, in the words the shell shows.
pub enum Ask {
    /// A book to open.
    Book { title: &'static str, filter: Filter },
    /// A folder to read in, or to search.
    Folder { title: String },
    /// One file of a book's own format, to relink the book by.
    File { title: &'static str, filter: Filter },
    /// Books to import, opening in a folder where one is known.
    Books {
        title: &'static str,
        filter: Filter,
        from: Option<PathBuf>,
    },
    /// Where to write `name`, as a `kind` (label, extension) file.
    Save {
        title: &'static str,
        name: String,
        kind: (&'static str, &'static str),
    },
}

/// What a dialog gave back.
pub enum Picked {
    /// Paths, on a machine that shows them; empty is a cancel.
    Paths(Vec<PathBuf>),
    /// A browser's file: its bytes, and the name it arrived with.
    Bytes(Vec<u8>, String),
}

impl Picked {
    /// The one path picked; none on a cancel, or where paths are hidden.
    fn path(self) -> Option<PathBuf> {
        match self {
            Self::Paths(paths) => paths.into_iter().next(),
            Self::Bytes(..) => None,
        }
    }
}

/// An answer from a dialog, given off the frame that asked for it.
pub type Done<T> = Box<dyn FnOnce(T) + Send>;

/// The dialogs a shell runs, shared by the front end that asks them.
pub type Runner = Arc<dyn Dialogs>;

/// The machine's own dialogs, which a front end asks and never runs.
///
/// A shell answers over whatever its platform picks files with, off the frame
/// that asked, and hands the pick back to `done`. False says the platform has
/// no such dialog at all, which the front end then reports in its own words.
pub trait Dialogs: Send + Sync {
    /// Asks `ask`, giving what comes back to `done`.
    fn ask(&self, ask: Ask, done: Done<Picked>) -> bool;
}

/// No machine to ask: a harness's, a shot's, and a test's.
pub struct Quiet;

impl Dialogs for Quiet {
    fn ask(&self, _ask: Ask, _done: Done<Picked>) -> bool {
        false
    }
}

/// A page's own picker, which the front end runs because a browser has no
/// shell beside it: files arrive as bytes, and folders do not arrive at all.
#[cfg(target_arch = "wasm32")]
pub struct Browser;

#[cfg(target_arch = "wasm32")]
impl Dialogs for Browser {
    fn ask(&self, ask: Ask, done: Done<Picked>) -> bool {
        let Ask::Book { title, filter } = ask else {
            return false;
        };
        wasm_bindgen_futures::spawn_local(async move {
            let dialog = rfd::AsyncFileDialog::new()
                .set_title(title)
                .add_filter(filter.0, filter.1);
            let picked = match dialog.pick_file().await {
                Some(file) => Picked::Bytes(file.read().await, file.file_name()),
                None => Picked::Paths(Vec::new()),
            };
            done(picked);
        });
        true
    }
}

/// What a front end asks before a shell has given it anything.
#[cfg(target_arch = "wasm32")]
fn runner() -> Runner {
    Arc::new(Browser)
}

/// Nothing on a desktop until its app hands over its own.
#[cfg(not(target_arch = "wasm32"))]
fn runner() -> Runner {
    Arc::new(Quiet)
}

/// Commands finished off the frame, waiting for the next one.
#[derive(Clone)]
pub struct Files {
    inbox: Arc<Mutex<Commands>>,
    /// One dialog at a time; a second request is ignored.
    picking: Arc<AtomicBool>,
    dialogs: Runner,
}

impl Default for Files {
    fn default() -> Self {
        Self {
            inbox: Arc::new(Mutex::new(Commands::new())),
            picking: Arc::new(AtomicBool::new(false)),
            dialogs: runner(),
        }
    }
}

impl Files {
    /// Answers its dialogs with `dialogs`, as a launch's shell does.
    pub fn set_dialogs(&mut self, dialogs: Runner) {
        self.dialogs = dialogs;
    }

    /// Moves arrived commands into this frame's queue.
    pub fn collect(&self, out: &mut Commands) {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        out.append(&mut inbox);
    }

    fn deliver(&self, waker: &Waker, command: Sent) {
        let mut inbox = self.inbox.lock().unwrap_or_else(PoisonError::into_inner);
        inbox.push(command);
        waker();
    }

    /// Shows the system open dialog; the pick lands as `placement` says.
    pub fn pick(&self, ctx: &Context, placement: Placement) {
        if self.picking.swap(true, Ordering::AcqRel) {
            return;
        }
        let (files, waker) = (self.clone(), wake(ctx));
        let done = move |picked: Picked| {
            files.picking.store(false, Ordering::Release);
            let request = match picked {
                Picked::Paths(paths) => paths.into_iter().next().map(OpenRequest::path),
                Picked::Bytes(bytes, name) => Some(OpenRequest::bytes(bytes, name)),
            };
            // Cancelling is silent, as in mareader.
            if let Some(request) = request {
                files.deliver(
                    &waker,
                    Sent::Engine(Command::Open(OpenRequest {
                        placement,
                        ..request
                    })),
                );
            }
        };
        let ask = Ask::Book {
            title: "Open a book",
            filter: ("Books", &BOOKS),
        };
        if !self.dialogs.ask(ask, Box::new(done)) {
            self.picking.store(false, Ordering::Release);
        }
    }

    /// Shows the folder dialog; `then` turns the pick into a command.
    pub fn pick_folder(
        &self,
        ctx: &Context,
        title: &str,
        then: impl FnOnce(PathBuf) -> Sent + Send + 'static,
    ) {
        let ask = Ask::Folder {
            title: title.to_owned(),
        };
        self.pick_paths(ctx, ask, first(then));
    }

    /// Shows the file dialog for one book's `extensions`.
    pub fn pick_book(
        &self,
        ctx: &Context,
        extensions: &'static [&'static str],
        then: impl FnOnce(PathBuf) -> Sent + Send + 'static,
    ) {
        let ask = Ask::File {
            title: "Choose the book's file",
            filter: ("This book's format", extensions),
        };
        self.pick_paths(ctx, ask, first(then));
    }

    /// Shows the file dialog for books to import, opening in `from`.
    pub fn pick_books(
        &self,
        ctx: &Context,
        from: Option<PathBuf>,
        then: impl FnOnce(Vec<PathBuf>) -> Sent + Send + 'static,
    ) {
        let ask = Ask::Books {
            title: "Import books into the library",
            filter: ("Books", &BOOKS),
            from,
        };
        let then = move |paths: Vec<PathBuf>| (!paths.is_empty()).then(|| then(paths));
        self.pick_paths(ctx, ask, then);
    }

    fn pick_paths(
        &self,
        ctx: &Context,
        ask: Ask,
        then: impl FnOnce(Vec<PathBuf>) -> Option<Sent> + Send + 'static,
    ) {
        if self.picking.swap(true, Ordering::AcqRel) {
            return;
        }
        let (files, waker) = (self.clone(), wake(ctx));
        let done = move |picked: Picked| {
            files.picking.store(false, Ordering::Release);
            let paths = match picked {
                Picked::Paths(paths) => paths,
                Picked::Bytes(..) => Vec::new(),
            };
            if let Some(command) = then(paths) {
                files.deliver(&waker, command);
            }
        };
        if !self.dialogs.ask(ask, Box::new(done)) {
            self.picking.store(false, Ordering::Release);
            let why = match cfg!(target_arch = "wasm32") {
                true => "Folders and relinks need the desktop app.",
                false => "The file dialog couldn't open.",
            };
            let toast = Toast::error(why);
            self.deliver(&wake(ctx), Sent::Ui(UiCommand::Notify(toast)));
        }
    }

    /// Asks where to save `contents` as `name`, then writes it there.
    pub fn save(&self, ctx: &Context, name: String, format: Format, contents: String) {
        let kind = (format.label(), format.extension());
        self.save_as(
            ctx,
            ("Export highlights", name),
            kind,
            contents.into_bytes(),
        );
    }

    /// Asks where to save `contents`, as a `kind` (label, extension) file.
    pub fn save_as(
        &self,
        ctx: &Context,
        (title, name): (&'static str, String),
        kind: (&'static str, &'static str),
        contents: Vec<u8>,
    ) {
        if self.picking.swap(true, Ordering::AcqRel) {
            return;
        }
        let ask = Ask::Save { title, name, kind };
        let (files, waker) = (self.clone(), wake(ctx));
        let done = move |picked: Picked| {
            files.picking.store(false, Ordering::Release);
            // Cancelling is silent.
            let Some(path) = picked.path() else { return };
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            let toast = match std::fs::write(&path, contents) {
                Ok(()) => Toast::info(format!("Exported to {name}")),
                Err(e) => Toast::error(format!("Couldn't export to {name}: {e}")),
            };
            files.deliver(&waker, Sent::Ui(UiCommand::Notify(toast)));
        };
        if !self.dialogs.ask(ask, Box::new(done)) {
            self.picking.store(false, Ordering::Release);
            let toast = Toast::error("Exporting needs the desktop app.");
            self.deliver(&wake(ctx), Sent::Ui(UiCommand::Notify(toast)));
        }
    }

    /// Opens a dropped file in `pane`.
    pub fn dropped(&self, ctx: &Context, file: &DroppedFileHandle, pane: PaneId) {
        let place = move |request: OpenRequest| OpenRequest {
            pane: Some(pane),
            ..request
        };
        read_drop(self, ctx, file, place);
    }
}

/// A command from a dialog's one pick; cancelling gives none.
fn first(
    then: impl FnOnce(PathBuf) -> Sent + Send + 'static,
) -> impl FnOnce(Vec<PathBuf>) -> Option<Sent> + Send + 'static {
    move |paths| paths.into_iter().next().map(then)
}

/// A desktop drop has a real path, so it opens at once.
#[cfg(not(target_arch = "wasm32"))]
fn read_drop(
    files: &Files,
    ctx: &Context,
    file: &DroppedFileHandle,
    place: impl FnOnce(OpenRequest) -> OpenRequest,
) {
    let request = place(OpenRequest::path(file.path().to_path_buf()));
    files.deliver(&wake(ctx), Sent::Engine(Command::Open(request)));
}

/// Browsers hide paths: files arrive as bytes, read asynchronously.
#[cfg(target_arch = "wasm32")]
fn read_drop(
    files: &Files,
    ctx: &Context,
    file: &DroppedFileHandle,
    place: impl FnOnce(OpenRequest) -> OpenRequest + 'static,
) {
    let (files, waker, file) = (files.clone(), wake(ctx), Arc::clone(file));
    wasm_bindgen_futures::spawn_local(async move {
        let name = file.path().to_string_lossy().into_owned();
        let command = match file.bytes_async().await {
            Ok(bytes) => Sent::Engine(Command::Open(place(OpenRequest::bytes(bytes, name)))),
            Err(e) => Sent::Ui(UiCommand::Notify(Toast::error(format!(
                "Couldn't read {name}: {e}"
            )))),
        };
        files.deliver(&waker, command);
    });
}
