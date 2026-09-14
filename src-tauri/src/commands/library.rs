//! The library's OS touch-points: walking a folder, measuring a file, copying
//! one into the app's store, and deleting a copy the app made.
//!
//! Deliberately raw IO and nothing else. Every *decision* — which files a
//! folder admits, what a rescan does about a file it has seen before, where a
//! book lands on a shelf — is `library_core`'s, running in the frontend where
//! the library state lives. This module answers the questions only a process
//! with a filesystem can answer, and hands back the measurements:
//!
//!   * [`scan_folder`] walks a tree and returns [`FoundFile`] rows, already
//!     filtered by the folder's own options, so a folder of forty thousand
//!     screenshots does not cross the wire;
//!   * [`verify_paths`] re-measures addresses the library already holds — what
//!     sets a book `missing`, and what replaces a migrated book's placeholder
//!     fingerprint with a real one;
//!   * [`store_books`] copies into `<app_data_dir>/Library/items/<id>/`, the only
//!     directory this module ever writes to, and stamps each copy with its own
//!     modification time so it measures as the file it is rather than as the
//!     one it came from ([`own_stamp`]);
//!   * [`delete_stored`] removes a copy and the book's own folder with it, and
//!     refuses anything outside the store;
//!   * [`relocate_stored`] moves a copy out of the old flat store into the
//!     book's own item folder, refusing either end that is not inside it;
//!   * [`reveal_in_folder`] hands a path to the OS file manager — the one
//!     verb here that neither measures nor writes, and the one with no
//!     document gate, because a shelf's DIRECTORY is as revealable as a
//!     book's file and opening a file manager on a path the reader pointed
//!     at is the whole of what it does.
//!
//! Progress is emitted on [`PROGRESS_EVENT`] rather than returned, because
//! walking a large folder takes longer than a UI is willing to look frozen.
//! Completion is NOT emitted: the frontend owns the task list, and a backend
//! that also declared "done" would be a second opinion about a state only one
//! side can see — a scan is usually followed by copies, and a "done" between
//! them would close the card early.
//!
//! The gates here are the crate's existing filesystem gate
//! (`ensure_readable_document`): these commands are reachable from a webview
//! that parses untrusted documents, so a path with no document suffix is
//! refused outright rather than measured, copied or deleted.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use std::io::Read as _;
use tauri::{AppHandle, Emitter, Manager};

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::hash::{HEAD_BYTES, head_hash, mtime_ms};
use library_core::scan::FoundFile;
use library_core::store;
// The wire types live in `library-core` because both sides of this IPC depend
// on it: one declaration, and no contract test needed to prove the halves agree.
use library_core::wire::{
    ImportPhase, ImportProgress, PathCheck, RelocateRequest, RelocateResult, StoreRequest,
    StoreResult,
};

/// The Tauri channel every progress beat is emitted on. The frontend's mirror
/// of this name lives in `src/services/library/mod.rs`, which re-broadcasts it as
/// window event so no component ever registers a Tauri listener of its own.
pub const PROGRESS_EVENT: &str = "library://progress";

/// How deep a walk descends. A tree deeper than this is either a loop this walk
/// did not catch or a directory nobody meant to import; either way the answer
/// is to stop rather than to keep going.
const MAX_DEPTH: usize = 12;

/// How many admitted files one scan returns before it gives up. A cap rather
/// than a guess about how big a library may be: past it the JSON crossing the
/// wire is larger than the state it would update, and the honest answer is to
/// ask for a narrower folder.
const MAX_FOUND: usize = 20_000;

/// Progress emits are batched: one every [`EMIT_EVERY`] files, or
/// [`EMIT_INTERVAL_MS`] since the last, whichever comes first. A 2 000-file
/// folder would otherwise push 2 000 messages through IPC in under a second,
/// and the frontend would spend the import repainting a ring.
const EMIT_EVERY: u32 = 8;
const EMIT_INTERVAL_MS: u128 = 60;

/// The throttle one command carries through its walk or its copies. The beat
/// it emits is [`ImportProgress`], shared with the frontend.
struct Progress {
    task: String,
    phase: ImportPhase,
    done: u32,
    total: u32,
    since_emit: u32,
    last: Instant,
}

impl Progress {
    fn new(task: &str, phase: ImportPhase, total: u32) -> Self {
        Self {
            task: task.to_string(),
            phase,
            done: 0,
            total,
            since_emit: 0,
            last: Instant::now(),
        }
    }

    /// Count one file, and emit when the batch is full or the interval has
    /// passed. A dropped emit is not an error: the next beat carries the same
    /// totals, and the final one is always flushed.
    fn tick(&mut self, app: &AppHandle, name: &str) {
        self.done = self.done.saturating_add(1);
        self.since_emit = self.since_emit.saturating_add(1);
        if self.since_emit < EMIT_EVERY && self.last.elapsed().as_millis() < EMIT_INTERVAL_MS {
            return;
        }
        self.flush(app, name);
    }

    fn flush(&mut self, app: &AppHandle, name: &str) {
        self.since_emit = 0;
        self.last = Instant::now();
        let _ = app.emit(
            PROGRESS_EVENT,
            &ImportProgress {
                task: self.task.clone(),
                phase: self.phase,
                done: self.done,
                total: self.total,
                name: name.to_string(),
            },
        );
    }
}

/// Walk `root` and measure every file the folder's options admit.
///
/// Runs on the blocking pool: a walk is a syscall per entry, and a large folder
/// would otherwise hold the async runtime for the whole import.
#[tauri::command]
pub async fn scan_folder(
    app: AppHandle,
    task: String,
    root: String,
    opts: FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    tauri::async_runtime::spawn_blocking(move || scan(&app, &task, &root, &opts))
        .await
        .map_err(|e| format!("scan worker failed: {e}"))?
}

/// One walk, and everything it accumulates. A struct rather than six arguments
/// threaded through a recursive call: the walk is the only place these are
/// touched, and a signature that has to be re-read to be called is a signature
/// that gets called wrong.
struct Scan<'a> {
    app: &'a AppHandle,
    root: &'a Path,
    opts: &'a FolderOpts,
    progress: Progress,
    found: Vec<FoundFile>,
    /// Set once [`MAX_FOUND`] is reached; every level above returns on it, so
    /// the walk unwinds instead of finishing the tree for nothing.
    truncated: bool,
}

impl Scan<'_> {
    /// One directory. Entries are sorted so books land in the order the folder
    /// itself lists them, which is what a reader expects a shelf to look like.
    fn walk(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH || self.truncated {
            return;
        }
        let Ok(read) = fs::read_dir(dir) else {
            // An unreadable directory is not a failed import: one subfolder's
            // permissions should not cost the reader the other ninety.
            return;
        };
        let mut entries: Vec<fs::DirEntry> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if self.truncated {
                return;
            }
            // `file_type` does not follow the link, which is the point: a
            // symlinked directory is a loop this walk has no business entering,
            // and a symlinked file's real address is somewhere else.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                // A hidden directory is somebody's cache, not a bookshelf.
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                self.walk(&path, depth + 1);
            } else if kind.is_file() {
                self.admit(entry, &path);
            }
        }
    }

    /// One file: measure it, ask the folder's options, and keep it if they say
    /// yes.
    fn admit(&mut self, entry: fs::DirEntry, path: &Path) {
        let Ok(meta) = entry.metadata() else {
            return;
        };
        let size = meta.len();
        let ext = extension_of(path);
        if !self.opts.admits_file(&ext, size) {
            return;
        }
        let fp = Fingerprint::of(size, mtime_ms(meta.modified().ok()), &read_head(path));
        let root = self.root;
        let found = FoundFile {
            rel: relative_to(root, path),
            path: path_to_string(path),
            ext,
            size,
            fp,
        };
        self.found.push(found);
        let label = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.progress.tick(self.app, &label);
        if self.found.len() >= MAX_FOUND {
            self.truncated = true;
        }
    }
}

fn scan(
    app: &AppHandle,
    task: &str,
    root: &str,
    opts: &FolderOpts,
) -> Result<Vec<FoundFile>, String> {
    let root_path = PathBuf::from(root);
    ensure_walkable(&root_path)?;
    let mut state = Scan {
        app,
        root: &root_path,
        opts,
        progress: Progress::new(task, ImportPhase::Scan, 0),
        found: Vec::new(),
        truncated: false,
    };
    state.walk(&root_path, 0);
    if state.truncated {
        return Err(format!(
            "This folder has more than {MAX_FOUND} documents — try importing a smaller folder."
        ));
    }
    // The last beat carries the real total, so the ring ends on the count
    // rather than on the throttle's last guess.
    let total = state.found.len() as u32;
    state.progress.total = total;
    state.progress.done = total;
    state.progress.flush(app, "");
    Ok(state.found)
}

/// Re-measure a list of addresses the library already holds.
///
/// One row per path asked about, in the order asked, so the caller can zip the
/// answer against its own list. A path that is refused by the document gate,
/// missing, or unreadable answers `exists: false` with zeroed measurements —
/// which is what turns a book `missing` rather than an error the UI has to
/// interpret.
#[tauri::command]
pub async fn verify_paths(paths: Vec<String>) -> Result<Vec<PathCheck>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        paths.iter().map(|path| check_path(path)).collect::<Vec<PathCheck>>()
    })
    .await
    .map_err(|e| format!("verify worker failed: {e}"))
}

fn check_path(path: &str) -> PathCheck {
    let missing = PathCheck {
        path: path.to_string(),
        exists: false,
        size: 0,
        mtime_ms: 0,
        head_hash: 0,
    };
    // The same gate the reader's own file reads pass through: "does this exist
    // and what are its first 8 KiB" is a question about ANY file, so it gets
    // the same answer the reader gets — documents only.
    if crate::ensure_readable_document(path).is_err() {
        return missing;
    }
    let p = Path::new(path);
    let Ok(meta) = fs::metadata(p) else {
        return missing;
    };
    if !meta.is_file() {
        return missing;
    }
    PathCheck {
        path: path.to_string(),
        exists: true,
        size: meta.len(),
        mtime_ms: mtime_ms(meta.modified().ok()),
        head_hash: head_hash(&read_head(p)),
    }
}

/// Copy files into the app's store, one result per request.
///
/// A failure is per-file rather than per-batch: a reader importing a folder
/// with one locked file in it should get the other ninety-nine, plus a line
/// naming the one that did not copy.
#[tauri::command]
pub async fn store_books(
    app: AppHandle,
    task: String,
    requests: Vec<StoreRequest>,
) -> Result<Vec<StoreResult>, String> {
    // No `?` here: the worker's own answer is already the whole result, so the
    // only error this can add is the worker failing to run at all.
    tauri::async_runtime::spawn_blocking(move || store(&app, &task, &requests))
        .await
        .map_err(|e| format!("store worker failed: {e}"))
}

fn store(app: &AppHandle, task: &str, requests: &[StoreRequest]) -> Vec<StoreResult> {
    let mut progress = Progress::new(task, ImportPhase::Copy, requests.len() as u32);
    let mut out = Vec::with_capacity(requests.len());
    for request in requests {
        let name = Path::new(&request.path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(copy_one(app, request, &mut progress, &name));
    }
    if !requests.is_empty() {
        progress.flush(app, "");
    }
    out
}

fn copy_one(
    app: &AppHandle,
    request: &StoreRequest,
    progress: &mut Progress,
    name: &str,
) -> StoreResult {
    let fail = |error: String| StoreResult {
        id: request.id.clone(),
        src: request.path.clone(),
        store: String::new(),
        error: Some(error),
    };
    if crate::ensure_readable_document(&request.path).is_err() {
        return fail(format!("not a document this app may copy: {}", request.path));
    }
    let target = match store_path(app, &request.path, &request.id) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the store directory: {e}"));
    }
    if let Err(e) = fs::copy(&request.path, &target) {
        return fail(format!("could not copy {}: {e}", request.path));
    }
    own_stamp(&target);
    progress.tick(app, name);
    StoreResult {
        id: request.id.clone(),
        src: request.path.clone(),
        store: path_to_string(&target),
        error: None,
    }
}

/// Give a fresh copy its own modification time.
///
/// The library measures a file as (size, modification time, first bytes), and
/// the whole of what a copy owes the ledger is a measurement of ITS OWN: a
/// stored row is known by its copy's fingerprint so the source file's stays
/// free for the folder that reads it. That is the departure rule's arithmetic,
/// and it is what lets a read-at-place book leave its shelf as a copy and come
/// back as a link without the library ever holding two rows it cannot tell
/// apart.
///
/// A copy inherits its source's bytes and size by definition, so the stamp is
/// the only one of the three that can differ — and `fs::copy` does not make it
/// differ everywhere. Linux leaves the copy with the time it was written;
/// Windows (`CopyFileExW`) and macOS (`fcopyfile` with `COPYFILE_STAT`) carry
/// the source's stamp across. On those two, an unstamped copy measures EXACTLY
/// like its source, the row adopts the source's fingerprint, and the folder's
/// ledger then reads its own copy as the file: a moved-out log is pruned as a
/// book that came back, a rescan relinks a provenance to itself on every window
/// focus, and an import of the source file lands a second copy beside the first
/// instead of bringing the linked book home.
///
/// Best-effort on purpose. A stamp the host refuses leaves the copy measurable
/// as its source, which the ledger's own copy rules survive; failing the copy
/// would lose the reader a book over a timestamp.
fn own_stamp(target: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(target) {
        let _ = file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()));
    }
}

/// Remove a file the app itself stored, and the book's folder with it.
///
/// The containment check is the whole safety story: the argument arrives from
/// the webview, and a delete primitive that trusted it would be `rm` with an IPC
/// wrapper. Only a path inside this app's own store directory is removed, and
/// the comparison is on canonicalised paths so a `..` cannot walk out. It is the
/// same rule [`relocate_stored`] answers with ([`inside_store`]), because a move
/// that could be talked into leaving the store would be a delete that could be
/// talked into anywhere.
///
/// The directory the file stood in goes with it. A stored book owns one folder
/// end to end ([`store::item_dir`]) — its bytes and whatever else the app keeps
/// beside them — and a removal that took only the file left a directory per
/// removed book under `items/` that no sweep would ever collect. A direct child
/// of `items/` is therefore removed whole; anything else in the store — a file
/// an older build left in the flat bucket — takes its directory only when it was
/// the last thing in it, so a bucket two books still share survives the first
/// one's removal. The store's own two directories are never a leaf's parent to
/// begin with.
#[tauri::command]
pub fn delete_stored(app: AppHandle, path: String) -> Result<(), String> {
    let root = store_root(&app)?;
    let target = PathBuf::from(&path);
    // The removal is of the RESOLVED path, not of the string that arrived. A
    // store file that is already gone is what the caller wanted, and the
    // containment rule answers it from the nearest ancestor that exists rather
    // than refusing a path it cannot canonicalise whole.
    let Some(target) = contained_in(&root, &target) else {
        return Err(format!("refusing to delete a file outside the store: {path}"));
    };
    match fs::remove_file(&target) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("could not delete {path}: {e}")),
    }
    sweep_the_books_folder(&root, &target);
    Ok(())
}

/// Take the directory a just-deleted store file stood in, by the two rules the
/// command's doc owns: a book's own item folder goes whole, and any other
/// directory goes only when the file was the last thing in it.
///
/// Best-effort and silent: a directory the host will not release is an empty
/// folder, not a removal that failed — the file the reader asked about is gone
/// either way, and there is no version of this where they should see an error
/// for it.
fn sweep_the_books_folder(root: &Path, deleted: &Path) {
    let Some(dir) = deleted.parent() else {
        return;
    };
    let Ok(root) = root.canonicalize() else {
        return;
    };
    if dir == root {
        return;
    }
    // `deleted` arrived out of `contained_in`, so it — and the parent this
    // reads — is a resolved path; the items root is resolved the same way
    // before the comparison, because a symlinked app-data directory would
    // otherwise fail a match the containment just proved.
    let items = PathBuf::from(store::items_root(&path_to_string(&root)));
    let is_item_dir = dir.parent().is_some_and(|grandparent| {
        items
            .canonicalize()
            .map_or(*grandparent == items, |real| *grandparent == real)
    });
    if is_item_dir {
        let _ = fs::remove_dir_all(dir);
    } else if fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_none()) {
        let _ = fs::remove_dir(dir);
    }
}

/// Reveal a path in the OS file manager: the item selected inside its folder
/// on the platforms that have the verb (macOS, Windows), the containing
/// folder opened on the platform that has not (Linux).
///
/// WHICH path a row reveals is the frontend's answer, not this one's — a book
/// the library copied names its copy in the store, a book read at its place
/// names the file where it stands, and a shelf of a watched folder names the
/// directory the tree cut it from. This is the hand-off to the OS and nothing
/// else: an existence check first, because the honest answer about a file
/// that is gone is a sentence here rather than a file manager opening on
/// nothing, and the spawn is the whole of the result — `explorer` answers
/// even a successful `/select` with a nonzero exit code, so waiting on a
/// status would report a failure for every success on Windows.
#[tauri::command]
pub async fn reveal_in_folder(path: String) -> Result<(), String> {
    let target = Path::new(&path);
    if !target.exists() {
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        return Err(format!("{name} is not there any more."));
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(target)
            .spawn()
            .map_err(|e| format!("The file manager did not open: {e}"))?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{path}"))
            .spawn()
            .map_err(|e| format!("The file manager did not open: {e}"))?;
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // No standard "select this file" verb: the containing folder is the
        // honest answer, and a directory reveals itself.
        let dir = if target.is_dir() {
            target.to_path_buf()
        } else {
            target
                .parent()
                .map_or_else(|| target.to_path_buf(), Path::to_path_buf)
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| format!("The file manager did not open: {e}"))?;
    }
    Ok(())
}

/// Move stored copies out of the old flat buckets and into their own item
/// folders, one result per request.
///
/// The store used to be `<root>/<format>/<stem>_<id>.<ext>`; it is now
/// `<root>/items/<id>/source.<ext>` ([`library_core::store`]). Copies made
/// before that change keep the address recorded in their row, so they still open
/// — but they sit in a layout nothing writes any more, with no folder of their
/// own for a cover or a set of marks to live in. This is the one-time move that
/// brings them across.
///
/// A MOVE and not a copy, because the bytes are already the app's own: the row
/// is the only thing that names them, and the frontend rewrites it from the
/// answer. Per-request rather than per-batch for the reason [`store_books`] is —
/// one copy a host will not let go of costs that book its old address, which
/// still opens, and not the other ninety-nine.
///
/// Both ends are held inside the store, on canonicalised paths, so a `..` cannot
/// walk out: `from` because a relocation is not a general file-move primitive
/// reachable from a webview that parses untrusted documents, and `to` because a
/// computed target that escaped would be a write the reader never asked for.
#[tauri::command]
pub async fn relocate_stored(
    app: AppHandle,
    requests: Vec<RelocateRequest>,
) -> Result<RelocateResult, String> {
    tauri::async_runtime::spawn_blocking(move || relocate(&app, &requests))
        .await
        .map_err(|e| format!("relocate worker failed: {e}"))
}

fn relocate(app: &AppHandle, requests: &[RelocateRequest]) -> RelocateResult {
    let root = match store_root(app) {
        Ok(root) => root,
        // No store root is no relocation: every row keeps the address it has,
        // which still opens. The empty root tells the frontend that nothing is a
        // candidate, so it does not ask again on the next launch either.
        Err(_) => {
            return RelocateResult {
                root: String::new(),
                results: requests.iter().map(|r| relocated_fail(r, NO_ROOT.to_string())).collect(),
            }
        }
    };
    let items = store::items_root(&path_to_string(&root));
    let mut results = Vec::with_capacity(requests.len());
    for request in requests {
        results.push(relocate_one(&root, &items, request));
    }
    RelocateResult {
        root: path_to_string(&root),
        results,
    }
}

/// The per-row error a shell with no app-data directory answers with.
const NO_ROOT: &str = "no store directory";

fn relocate_one(root: &Path, items: &str, request: &RelocateRequest) -> StoreResult {
    let fail = |error: String| relocated_fail(request, error);
    let source = Path::new(&request.from);
    if !inside_store(root, source) {
        return fail(format!("refusing to move a file outside the store: {}", request.from));
    }
    let ext = extension_of(source).to_lowercase();
    let Some(ext) = store::migrated_ext(&ext) else {
        return fail(format!("not a format this app stores: {}", request.from));
    };
    let target = PathBuf::from(store::source_path(items, &request.id, ext));
    // Containment is asked of the path itself and not of the resolved one: a
    // `..` the id carried is refused here even when the file it names happens to
    // land back inside the store through a symlink, which is the difference
    // between a rule about the address and a rule about where it ended up.
    if !inside_store(root, &target) {
        return fail(format!("refusing to write outside the store: {}", request.id));
    }
    // Already where it belongs: a second launch, or a row this pass moved. The
    // answer is the address it already wears rather than an error, so a
    // migration that runs twice is a migration that did nothing the second time.
    if same_file(source, &target) {
        return StoreResult {
            id: request.id.clone(),
            src: request.from.clone(),
            store: path_to_string(&target),
            error: None,
        };
    }
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the item directory: {e}"));
    }
    // A rename is a metadata edit on one volume, and the two ends are both
    // inside the store root — but an app-data directory that is itself a symlink
    // onto another volume makes that a cross-device rename, which the host
    // refuses. The fallback is the same bytes either way: copy, then remove the
    // source so the old bucket is not left holding a file nothing points at.
    // The copy is only attempted when the rename left nothing behind, so a
    // rename that failed for a real reason is reported rather than papered over.
    if let Err(e) = fs::rename(source, &target) {
        if target.exists() {
            return fail(format!("could not move {}: {e}", request.from));
        }
        if let Err(e) = fs::copy(source, &target) {
            return fail(format!("could not move {}: {e}", request.from));
        }
        // The copy landed, so the row may move. A source nobody will let go of
        // is a file left in a bucket nothing reads any more — wasted disk, and a
        // better trade than losing the reader the book over it.
        let _ = fs::remove_file(source);
    }
    // The move does not write the file, so the stamp a copy needed is already
    // the one the book was stored with. Leaving it alone is the point: the row's
    // identity is the measurement of THESE bytes, and re-stamping on a migration
    // would change a fingerprint every ledger entry and tombstone still names.
    StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: path_to_string(&target),
        error: None,
    }
}

fn relocated_fail(request: &RelocateRequest, error: String) -> StoreResult {
    StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: String::new(),
        error: Some(error),
    }
}

/// Whether two addresses are the same file, canonicalised so a path that differs
/// only in how it was spelled does not read as a move onto itself.
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Whether `path` is inside the store root, compared on canonicalised paths so a
/// `..` cannot walk out.
///
/// The nearest EXISTING ancestor is what gets canonicalised, and the rest of the
/// path is appended to it, because one of the two ends of a move has not been
/// created yet: `canonicalize` refuses a path that is not there, and a target
/// that always answered "outside" would make the migration refuse every book it
/// was asked to move. Walking up to something real keeps the comparison honest —
/// a symlinked store root resolves, and a `..` in the tail is still resolved
/// against the real directory it sits in.
///
/// `false` when nothing on the path exists at all, which is a refusal rather
/// than a guess: a file that is gone cannot be moved either.
fn inside_store(root: &Path, path: &Path) -> bool {
    contained_in(root, path).is_some()
}

/// `path` resolved against `root`, or `None` when it is not inside it.
///
/// The nearest EXISTING ancestor is what gets canonicalised and the rest of the
/// path is resolved lexically on top of it, because one end of a move has not
/// been created yet: `canonicalize` refuses a path that is not there, and a
/// target that always answered "outside" would refuse every book the migration
/// was asked to move. Splitting at the ancestor that is real keeps both halves
/// honest — a symlinked store root resolves, and a `..` in the tail is honoured
/// against the real directory it sits in rather than appended to it, so a
/// computed path that tried to climb out is still refused.
///
/// `None` when the root does not exist or nothing on the path does, which is a
/// refusal rather than a guess.
fn contained_in(root: &Path, path: &Path) -> Option<PathBuf> {
    let root = root.canonicalize().ok()?;
    let mut existing = path;
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let base = loop {
        if let Ok(real) = existing.canonicalize() {
            break real;
        }
        let (Some(parent), Some(name)) = (existing.parent(), existing.file_name()) else {
            return None;
        };
        tail.push(name);
        existing = parent;
    };
    let mut full = base;
    for name in tail.iter().rev() {
        if *name == std::ffi::OsStr::new("..") {
            full = full.parent()?.to_path_buf();
        } else if *name != std::ffi::OsStr::new(".") {
            full.push(name);
        }
    }
    full.starts_with(&root).then_some(full)
}

/// `<app_data_dir>/Library` — the only directory this module writes to.
fn store_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("Library"))
        .map_err(|e| format!("no app data directory: {e}"))
}

/// Where one source file lands in the store: `<root>/items/<id>/source.<ext>`.
///
/// One folder per book, keyed by the id that never changes — the layout
/// [`library_core::store`] owns and the reason a copy is the only thing a store
/// write puts there at first (a book's cover and marks join it later, in the
/// same folder). Nothing on disk is named after the source file's stem, so a
/// rename never touches the filesystem and two books both called `report.pdf`
/// cannot collide; the id is sanitised into its folder name by the crate rather
/// than trusted here.
fn store_path(app: &AppHandle, src: &str, id: &str) -> Result<PathBuf, String> {
    let root = store_root(app)?;
    let items = store::items_root(&path_to_string(&root));
    let ext = extension_of(Path::new(src));
    Ok(PathBuf::from(store::source_path(&items, id, &ext)))
}

/// The lower-case extension of a path, without its dot. Empty for a name Rust
/// reads as having none (`Makefile`, and a dotfile like `.gitignore`, whose
/// leading dot does not start an extension) — which the format registry also
/// refuses, so an extension-less file is never admitted, measured or copied.
fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

/// `path` relative to `root`, with `/` separators on every platform. The
/// subfolder half of this string is what a grouped import cuts its shelves
/// from, so it is normalised here rather than at three call sites.
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        .replace('\\', "/")
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// The first [`HEAD_BYTES`] of a file, or nothing when it cannot be read. An
/// unreadable head is not a failed scan: the size and the stamp still identify
/// the file, and a book that opens is worth more than a hash that is exact.
fn read_head(path: &Path) -> Vec<u8> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(HEAD_BYTES as u64).read_to_end(&mut buf);
    buf
}

/// The gate on a walk's starting point: absolute, and a directory. Not the
/// document-suffix gate — a folder is not a document — but the same refusal of
/// a relative path the crate's other filesystem commands apply.
fn ensure_walkable(root: &Path) -> Result<(), String> {
    let text = root.to_string_lossy();
    if !crate::path_looks_absolute(&text) {
        return Err(format!("refusing to walk a relative path: {text}"));
    }
    match fs::metadata(root) {
        Ok(meta) if meta.is_dir() => Ok(()),
        Ok(_) => Err(format!("not a directory: {text}")),
        Err(e) => Err(format!("cannot read {text}: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        extension_of, inside_store, own_stamp, relative_to, same_file, sweep_the_books_folder,
    };
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, SystemTime};

    #[test]
    fn an_extension_is_lower_case_and_has_no_dot() {
        assert_eq!(extension_of(Path::new("/books/Dune.PDF")), "pdf");
        assert_eq!(extension_of(Path::new("/books/notes.markdown")), "markdown");
        assert_eq!(extension_of(Path::new("/books/Makefile")), "");
        // Rust reads a dotfile as having no extension, and no extension is
        // nothing the format registry admits — the two agree without a rule.
        assert_eq!(extension_of(Path::new("/books/.gitignore")), "");
    }

    /// The two refusals that keep a relocation inside the app's own store, on
    /// canonicalised paths so a `..` cannot walk out — and the agreement about
    /// "already there", which is what makes a migration that runs twice a
    /// migration that did nothing the second time.
    #[test]
    fn a_relocation_stays_inside_the_store() {
        let dir = std::env::temp_dir().join(format!("pdf-reader-move-{}", std::process::id()));
        let store = dir.join("Library");
        let bucket = store.join("pdf");
        let outside = dir.join("books");
        fs::create_dir_all(&bucket).expect("a scratch store");
        fs::create_dir_all(&outside).expect("a scratch folder");
        let copy = bucket.join("dune_ab12.pdf");
        fs::write(&copy, b"%PDF-1.7 dune").expect("a scratch copy");
        let reader_file = outside.join("dune.pdf");
        fs::write(&reader_file, b"%PDF-1.7 the reader's own").expect("a scratch file");

        assert!(inside_store(&store, &copy), "a stored copy is inside");
        assert!(
            !inside_store(&store, &reader_file),
            "the reader's own file is not, however document-shaped it is"
        );
        // A traversal that lands outside is refused on the canonicalised path,
        // not on the string that was handed over.
        let escape = store.join("..").join("books").join("dune.pdf");
        assert!(!inside_store(&store, &escape));
        // A target that has not been created yet is the half a move owes, so the
        // answer comes from the nearest ancestor that IS there. Refusing it would
        // refuse every book the migration was asked to move.
        let target = store.join("items").join("b018c4f9e2a0").join("source.pdf");
        assert!(!target.exists(), "the target is the file this move would make");
        assert!(inside_store(&store, &target), "and it is inside all the same");
        // A computed target that climbs out is refused on the canonicalised
        // ancestor, not on the string that was handed over.
        assert!(!inside_store(&store, &store.join("items").join("..").join("..").join("etc")));

        // Same file, two spellings: the migration must read this as "already
        // moved" rather than as a move onto itself.
        assert!(same_file(&copy, &copy));
        assert!(same_file(&copy, &store.join("pdf").join("dune_ab12.pdf")));
        assert!(!same_file(&copy, &reader_file));
        // Neither end there is still an answer rather than a panic.
        assert!(!same_file(&bucket.join("a.pdf"), &bucket.join("b.pdf")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_always_uses_forward_slashes() {
        // The subfolder a shelf is cut from is matched against a persisted
        // ledger key, so it cannot be platform-shaped.
        let root = Path::new("/books");
        assert_eq!(relative_to(root, Path::new("/books/a.pdf")), "a.pdf");
        assert_eq!(
            relative_to(root, Path::new("/books/scifi/deep/a.pdf")),
            "scifi/deep/a.pdf"
        );
        // A path that is not under the root at all still answers with something
        // usable rather than an empty string that would name the root's shelf.
        assert_eq!(relative_to(root, Path::new("/other/a.pdf")), "/other/a.pdf");
    }

    /// The stamp is what separates a copy's measurement from its source's, and
    /// two of the three hosts this ships on carry the source's stamp across a
    /// copy. So: write a file, backdate it the way a source is backdated by
    /// having been written last week, and ask for the copy's own stamp — which
    /// is the whole of what the ledger's copy rules need.
    ///
    /// The backdating is `own_stamp`'s own mechanism, so a host that cannot set
    /// a stamp cannot run this test and says so rather than passing vacuously.
    #[test]
    fn a_copy_takes_its_own_modification_time() {
        let dir = std::env::temp_dir().join(format!("pdf-reader-stamp-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("a scratch directory");
        let path = dir.join("book.pdf");
        fs::write(&path, b"%PDF-1.7 a book").expect("a scratch file");

        let backdated = SystemTime::now() - Duration::from_secs(7 * 24 * 3600);
        let file = fs::File::options().write(true).open(&path).expect("a handle");
        file.set_times(fs::FileTimes::new().set_modified(backdated))
            .expect("a host that sets a stamp");
        drop(file);
        let before = fs::metadata(&path).and_then(|m| m.modified()).expect("a stamp to read");
        assert_eq!(
            before, backdated,
            "the backdating took, so the assertion below means something"
        );

        own_stamp(&path);

        let after = fs::metadata(&path).and_then(|m| m.modified()).expect("a stamp to read");
        assert!(
            after > before + Duration::from_secs(3600),
            "the stamp is the copy's own, not the source's week-old one"
        );
        assert!(
            SystemTime::now().duration_since(after).unwrap_or_default() < Duration::from_secs(3600),
            "and it is a stamp of this run, not a wrapped or invented one"
        );
        let _ = fs::remove_file(&path);
        let _ = fs::remove_dir(&dir);
    }

    /// A stamp nobody can set is not a failed copy: the file is there, and the
    /// ledger's own copy rules carry a measurement that cannot tell the two
    /// apart. The promise is only that a refusal does not panic.
    #[test]
    fn a_stamp_nobody_can_set_is_not_an_error() {
        own_stamp(Path::new("/this/path/is/not/there/book.pdf"));
    }

    /// The folder a removed store book stood in goes with it — whole, when it
    /// is the book's own item folder, and only when empty for anything else —
    /// while the store's own two directories outlive every leaf. The paths are
    /// resolved before the sweep the way the command resolves them, because
    /// the containment the sweep trusts is `contained_in`'s answer, and a
    /// temp directory under a symlinked `/tmp` is exactly where an unresolved
    /// comparison would pass on one platform and fail on the next.
    #[test]
    fn a_removed_book_takes_its_own_folder_and_nothing_above_it() {
        let root = std::env::temp_dir().join(format!("pdf-reader-sweep-{}", std::process::id()));
        let item = root.join("items").join("b1");
        let legacy = root.join("pdf");
        fs::create_dir_all(&item).expect("an item folder");
        fs::create_dir_all(&legacy).expect("a legacy bucket");
        let source = item.join("source.pdf");
        let one = legacy.join("one.pdf");
        let two = legacy.join("two.pdf");
        let loose = root.join("loose.pdf");
        for path in [&source, &one, &two, &loose] {
            fs::write(path, b"%PDF-1.7 a book").expect("a scratch file");
        }

        // The item folder goes whole, cover debris and all: the book owns it.
        // The resolution happens while the file exists, the way the command's
        // containment resolves it before the removal.
        fs::write(item.join("cover.webp"), b"art").expect("a stand-in cover");
        let resolved = source.canonicalize().expect("a resolution");
        fs::remove_file(&source).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(!item.exists(), "the book's folder left with the book");
        assert!(root.join("items").exists(), "the items root is the store's own");

        // A shared legacy bucket survives the first of its two files...
        let resolved = one.canonicalize().expect("a resolution");
        fs::remove_file(&one).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(legacy.exists(), "a bucket two books shared keeps the second");
        // ...and goes when the second was the last thing in it.
        let resolved = two.canonicalize().expect("a resolution");
        fs::remove_file(&two).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(!legacy.exists(), "an empty bucket is nobody's");

        // A file at the store root leaves the root itself alone.
        let resolved = loose.canonicalize().expect("a resolution");
        fs::remove_file(&loose).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(root.exists(), "the store root is never a leaf's folder to take");

        let _ = fs::remove_dir_all(&root);
    }
}
