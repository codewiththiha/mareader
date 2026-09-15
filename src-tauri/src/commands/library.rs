//! The library's OS touch-points: walking a folder, measuring a file, copying
//! one into the app's store, and deleting a copy the app made.
//!
//! Raw IO and nothing else. Every decision — which files a folder admits,
//! what a rescan does about a file it has seen, where a book lands on a
//! shelf — is `library_core`'s, running in the frontend where the library
//! state lives.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime};

use std::io::Read as _;
use tauri::{AppHandle, Emitter, Manager};

use library_core::book::Fingerprint;
use library_core::folder::FolderOpts;
use library_core::hash::{HEAD_BYTES, head_hash, mtime_ms};
use library_core::paths;
use library_core::scan::FoundFile;
use library_core::store;
use library_core::wire::{
    BookFileRequest, ImportPhase, ImportProgress, PathCheck, RelocateResult, StoreResult,
};

/// Mirrored by the frontend in `src/services/library/mod.rs`, which folds it
/// into the dock's task list so no component registers a Tauri listener of
/// its own.
pub const PROGRESS_EVENT: &str = "library://progress";

/// A tree deeper than this is either a loop this walk did not catch or a
/// directory nobody meant to import; either way the answer is to stop.
const MAX_DEPTH: usize = 12;

/// Past this the JSON crossing the wire is larger than the state it would
/// update, and the honest answer is to ask for a narrower folder.
const MAX_FOUND: usize = 20_000;

/// A 2 000-file folder would otherwise push 2 000 messages through IPC in
/// under a second, and the frontend would spend the import repainting a ring.
const EMIT_EVERY: u32 = 8;
const EMIT_INTERVAL_MS: u128 = 60;

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

    /// A dropped emit is not an error: the next beat carries the same totals
    /// and the final one is always flushed. The first file emits too — a
    /// three-file import that showed nothing until its final flush would read
    /// as a hang — and after that the throttle holds.
    fn tick(&mut self, app: &AppHandle, name: &str) {
        self.done = self.done.saturating_add(1);
        self.since_emit = self.since_emit.saturating_add(1);
        if self.done > 1 && self.since_emit < EMIT_EVERY && self.last.elapsed().as_millis() < EMIT_INTERVAL_MS {
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

/// Runs on the blocking pool: a walk is a syscall per entry, and a large
/// folder would otherwise hold the async runtime for the whole import.
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

struct Scan<'a> {
    app: &'a AppHandle,
    root: &'a Path,
    opts: &'a FolderOpts,
    progress: Progress,
    found: Vec<FoundFile>,
    truncated: bool,
}

impl Scan<'_> {
    fn walk(&mut self, dir: &Path, depth: usize) {
        if depth > MAX_DEPTH || self.truncated {
            return;
        }
        let Ok(read) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<fs::DirEntry> = read.filter_map(Result::ok).collect();
        entries.sort_by_key(fs::DirEntry::file_name);

        for entry in entries {
            if self.truncated {
                return;
            }
            // `file_type` does not follow the link, which is the point: a
            // symlinked directory is a loop this walk has no business
            // entering.
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue;
            }
            let path = entry.path();
            if kind.is_dir() {
                if entry.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                self.walk(&path, depth + 1);
            } else if kind.is_file() {
                self.admit(entry, &path);
            }
        }
    }

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
    let total = state.found.len() as u32;
    state.progress.total = total;
    state.progress.done = total;
    state.progress.flush(app, "");
    Ok(state.found)
}

/// One row per path asked about, in the order asked, so the caller can zip
/// the answer against its own list. A path refused by the document gate,
/// missing or unreadable answers `exists: false` with zeroed measurements.
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

/// A failure is per-file rather than per-batch: a folder with one locked
/// file in it still imports the other ninety-nine, plus a line naming the one
/// that did not copy.
#[tauri::command]
pub async fn store_books(
    app: AppHandle,
    task: String,
    requests: Vec<BookFileRequest>,
) -> Result<Vec<StoreResult>, String> {
    tauri::async_runtime::spawn_blocking(move || store(&app, &task, &requests))
        .await
        .map_err(|e| format!("store worker failed: {e}"))
}

fn store(app: &AppHandle, task: &str, requests: &[BookFileRequest]) -> Vec<StoreResult> {
    let mut progress = Progress::new(task, ImportPhase::Copy, requests.len() as u32);
    let mut out = Vec::with_capacity(requests.len());
    for request in requests {
        let name = Path::new(&request.from)
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
    request: &BookFileRequest,
    progress: &mut Progress,
    name: &str,
) -> StoreResult {
    let fail = |error: String| StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: String::new(),
        error: Some(error),
        measured: None,
    };
    if crate::ensure_readable_document(&request.from).is_err() {
        return fail(format!("not a document this app may copy: {}", request.from));
    }
    let target = match store_path(app, &request.from, &request.id) {
        Ok(t) => t,
        Err(e) => return fail(e),
    };
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the store directory: {e}"));
    }
    if let Err(e) = fs::copy(&request.from, &target) {
        return fail(format!("could not copy {}: {e}", request.from));
    }
    own_stamp(&target);
    progress.tick(app, name);
    StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: path_to_string(&target),
        error: None,
        // Measured by the same pass that stamped it: the row lands wearing
        // its copy's own identity, and the folder that reads the source keeps
        // the source's free.
        measured: Some(measure_copy(&target)),
    }
}

/// The copy's own (size, mtime, first bytes) — the same triple
/// [`check_path`] reads — taken here so the answer rides home with the copy
/// instead of costing a second trip.
fn measure_copy(target: &Path) -> Fingerprint {
    let Ok(meta) = fs::metadata(target) else {
        return Fingerprint::of(0, 0, &[]);
    };
    Fingerprint::of(meta.len(), mtime_ms(meta.modified().ok()), &read_head(target))
}

/// A copy owes the ledger a measurement of its own: a stored row is known by
/// its copy's fingerprint, so the source file's stays free for the folder
/// that reads it.
fn own_stamp(target: &Path) {
    if let Ok(file) = fs::File::options().write(true).open(target) {
        let _ = file.set_times(fs::FileTimes::new().set_modified(SystemTime::now()));
    }
}

/// The containment check is the whole safety story: the argument arrives
/// from the webview, and a delete primitive that trusted it would be `rm`
/// with an IPC wrapper. Only a path inside this app's own store directory is
/// removed, on canonicalised paths so a `..` cannot walk out.
///
/// Async like every other fs command here: a sync command runs on the main
/// thread, and a slow disk holding the sweep would freeze the UI.
#[tauri::command]
pub async fn delete_stored(app: AppHandle, path: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || delete(&app, &path))
        .await
        .map_err(|e| format!("delete worker failed: {e}"))?
}

fn delete(app: &AppHandle, path: &str) -> Result<(), String> {
    let root = store_root(app)?;
    let target = PathBuf::from(path);
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

/// A book's own item folder goes whole, and any other directory goes only when the file was
/// the last thing in it. Silent: a directory the host will not release is an empty folder, not a
/// removal that failed.
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
    // The items root is resolved the same way before the comparison, because a symlinked app-data directory would otherwise fail a match the containment just proved.
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

/// Which path a row reveals is the frontend's answer, not this one's: a
/// copied book names its copy in the store, a read-at-place book names the
/// file where it stands.
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

/// The store used to be `<root>/<format>/<stem>_<id>.<ext>`; it is now
/// `<root>/items/<id>/source.<ext>` ([`library_core::store`]). Copies made before that change
/// keep the address recorded in their row, so they still open — but they sit in a layout
/// nothing writes any more.
#[tauri::command]
pub async fn relocate_stored(
    app: AppHandle,
    requests: Vec<BookFileRequest>,
) -> Result<RelocateResult, String> {
    tauri::async_runtime::spawn_blocking(move || relocate(&app, &requests))
        .await
        .map_err(|e| format!("relocate worker failed: {e}"))
}

fn relocate(app: &AppHandle, requests: &[BookFileRequest]) -> RelocateResult {
    let root = match store_root(app) {
        Ok(root) => root,
        Err(_) => {
            return RelocateResult {
                root: String::new(),
                results: requests
                    .iter()
                    .map(|r| StoreResult {
                        id: r.id.clone(),
                        src: r.from.clone(),
                        store: String::new(),
                        error: Some(NO_ROOT.to_string()),
                        measured: None,
                    })
                    .collect(),
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

const NO_ROOT: &str = "no store directory";

fn relocate_one(root: &Path, items: &str, request: &BookFileRequest) -> StoreResult {
    let fail = |error: String| StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: String::new(),
        error: Some(error),
        // A failed move answers with no measurement, exactly as a failed copy does.
        measured: None,
    };
    let source = Path::new(&request.from);
    if !inside_store(root, source) {
        return fail(format!("refusing to move a file outside the store: {}", request.from));
    }
    let ext = extension_of(source).to_lowercase();
    let Some(ext) = store::migrated_ext(&ext) else {
        return fail(format!("not a format this app stores: {}", request.from));
    };
    let target = PathBuf::from(store::source_path(items, &request.id, ext));
    if !inside_store(root, &target) {
        return fail(format!("refusing to write outside the store: {}", request.id));
    }
    if same_file(source, &target) {
        return StoreResult {
            id: request.id.clone(),
            src: request.from.clone(),
            store: path_to_string(&target),
            error: None,
            measured: None,
        };
    }
    if let Some(parent) = target.parent()
        && let Err(e) = fs::create_dir_all(parent)
    {
        return fail(format!("could not create the item directory: {e}"));
    }
    // An app-data directory that is itself a symlink onto another volume
    // makes a rename cross-device, which the host refuses: copy, then remove
    // the source so the old bucket is not left holding a file nothing points
    // at. A source the host will not release is reported rather than
    // swallowed — the ledger would otherwise believe the book lives in one
    // place while a second copy sits in the other.
    if let Err(e) = fs::rename(source, &target) {
        if target.exists() {
            return fail(format!("could not move {}: {e}", request.from));
        }
        if let Err(e) = fs::copy(source, &target) {
            return fail(format!("could not move {}: {e}", request.from));
        }
        if let Err(e) = fs::remove_file(source) {
            return fail(format!(
                "copied to the new store, but the old copy stayed behind: {e}"
            ));
        }
    }
    // The row's identity is the measurement of these bytes; re-stamping on a
    // migration would change a fingerprint every ledger entry and tombstone
    // still names — which is why `measured` stays `None` here.
    StoreResult {
        id: request.id.clone(),
        src: request.from.clone(),
        store: path_to_string(&target),
        error: None,
        measured: None,
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => a == b,
    }
}

/// Canonicalised so a `..` cannot walk out; see [`contained_in`] for the
/// not-yet-created end of a move.
fn inside_store(root: &Path, path: &Path) -> bool {
    contained_in(root, path).is_some()
}

/// The nearest existing ancestor canonicalised, the rest resolved lexically
/// on top of it: `canonicalize` refuses a path that is not there, and a
/// target that always answered "outside" would refuse every book the
/// migration was asked to move.
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

fn store_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_data_dir()
        .map(|dir| dir.join("Library"))
        .map_err(|e| format!("no app data directory: {e}"))
}

/// One folder per book, keyed by the id that never changes: nothing on disk
/// is named after the source file's stem, so a rename never touches the file.
fn store_path(app: &AppHandle, src: &str, id: &str) -> Result<PathBuf, String> {
    let root = store_root(app)?;
    let items = store::items_root(&path_to_string(&root));
    let ext = extension_of(Path::new(src));
    Ok(PathBuf::from(store::source_path(&items, id, &ext)))
}

/// Empty for a name Rust reads as having none (`Makefile`, `.gitignore`) —
/// which the format registry also refuses, so an extension-less file is
/// never admitted, measured or copied. The spelling is
/// [`library_core::paths::extension`]'s, so a `Path` here and a string in
/// the frontend answer the same.
fn extension_of(path: &Path) -> String {
    paths::extension(&path.to_string_lossy())
}

/// The subfolder half of this string is what a grouped import cuts its
/// shelves from, so it is normalised here rather than at three call sites.
fn relative_to(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        .replace('\\', "/")
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// An unreadable head is not a failed scan: the size and the stamp still
/// identify the file, and a book that opens is worth more than a hash that
/// is exact.
fn read_head(path: &Path) -> Vec<u8> {
    let Ok(file) = fs::File::open(path) else {
        return Vec::new();
    };
    let mut buf = Vec::new();
    let _ = file.take(HEAD_BYTES as u64).read_to_end(&mut buf);
    buf
}

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
        assert_eq!(extension_of(Path::new("/books/.gitignore")), "");
    }

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
        let escape = store.join("..").join("books").join("dune.pdf");
        assert!(!inside_store(&store, &escape));
        let target = store.join("items").join("b018c4f9e2a0").join("source.pdf");
        assert!(!target.exists(), "the target is the file this move would make");
        assert!(inside_store(&store, &target), "and it is inside all the same");
        assert!(!inside_store(&store, &store.join("items").join("..").join("..").join("etc")));

        assert!(same_file(&copy, &copy));
        assert!(same_file(&copy, &store.join("pdf").join("dune_ab12.pdf")));
        assert!(!same_file(&copy, &reader_file));
        assert!(!same_file(&bucket.join("a.pdf"), &bucket.join("b.pdf")));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_relative_path_always_uses_forward_slashes() {
        let root = Path::new("/books");
        assert_eq!(relative_to(root, Path::new("/books/a.pdf")), "a.pdf");
        assert_eq!(
            relative_to(root, Path::new("/books/scifi/deep/a.pdf")),
            "scifi/deep/a.pdf"
        );
        assert_eq!(relative_to(root, Path::new("/other/a.pdf")), "/other/a.pdf");
    }

    /// Two of the three hosts this ships on carry the source's stamp across
    /// a copy, so the copy's own stamp is what the ledger's rules need.
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

    #[test]
    fn a_stamp_nobody_can_set_is_not_an_error() {
        own_stamp(Path::new("/this/path/is/not/there/book.pdf"));
    }

    /// Paths are resolved before the sweep the way the command resolves
    /// them: the containment the sweep trusts is `contained_in`'s answer.
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

        fs::write(item.join("cover.webp"), b"art").expect("a stand-in cover");
        let resolved = source.canonicalize().expect("a resolution");
        fs::remove_file(&source).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(!item.exists(), "the book's folder left with the book");
        assert!(root.join("items").exists(), "the items root is the store's own");

        let resolved = one.canonicalize().expect("a resolution");
        fs::remove_file(&one).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(legacy.exists(), "a bucket two books shared keeps the second");
        let resolved = two.canonicalize().expect("a resolution");
        fs::remove_file(&two).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(!legacy.exists(), "an empty bucket is nobody's");

        let resolved = loose.canonicalize().expect("a resolution");
        fs::remove_file(&loose).expect("a removal");
        sweep_the_books_folder(&root, &resolved);
        assert!(root.exists(), "the store root is never a leaf's folder to take");

        let _ = fs::remove_dir_all(&root);
    }
}
