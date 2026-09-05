//! PDF Reader — Tauri backend.
//!
//! Besides hosting the webview this crate owns the two OS touch-points a
//! desktop reader needs and the frontend cannot do itself:
//!
//!   * `read_file_bytes` — binary IPC fallback when the asset protocol cannot
//!     serve the file. Returned as an `ArrayBuffer` (not JSON/Base64). The
//!     frontend first tries `convertFileSrc` + Range fetch so pdf.js can
//!     stream; Windows `https://asset.localhost` still intermittently fails
//!     ("Failed to fetch"), so this command remains the reliable path.
//!
//!   * OS file opening — double-click / "Open with" / default-app launch.
//!     The file path arrives differently per platform, so all three routes
//!     queue into one `PendingFile` slot:
//!       - macOS   : `RunEvent::Opened` (LaunchServices), at launch AND while
//!         running (the running instance receives it).
//!       - Windows/Linux initial launch : plain argv entry ("%1" from the
//!         shell association).
//!       - Windows/Linux second launch : `tauri-plugin-single-instance`
//!         forwards the second process's argv to the running one
//!         instead of spawning a second window.
//!
//!     The frontend collects the slot through the `take_pending_file`
//!     command (the authoritative handoff — an event emitted before the
//!     webview mounted would otherwise be lost) and `document-open-file` is only
//!     the wake-up ping for files that arrive while it is already mounted.

use std::sync::Mutex;

use tauri::{Emitter, Manager, RunEvent};

mod ai;
mod commands;
mod macos;

/// Every extension the reader opens (lower-case, dot included). The shell's
/// filesystem gates accept exactly these and nothing else, so the webview's
/// `read_file_*` commands are not a general file-read primitive.
///
/// Derived from the frontend's `reader_core::format` registry, which is the
/// source of truth — `tools/check-formats.ts` fails CI when the two drift, as
/// it does for the bundle's file associations in `tauri.conf.json`.
const DOCUMENT_EXTENSIONS: &[&str] = &[".pdf", ".txt", ".text", ".md", ".markdown", ".mdown"];

/// The OS-opened document path the frontend has not collected yet.
struct PendingFile(Mutex<Option<String>>);

/// True for anything we should try to open as a document.
///
/// Also strips Windows shell quoting: `std::env::args()` on Windows does not
/// remove the quotes Explorer puts around `%1`, so `"C:\My Docs\a.pdf"`
/// arrives quoted and would fail the suffix test.
fn is_document_path(raw: &str) -> bool {
    let p = raw.trim().trim_matches('"').to_lowercase();
    DOCUMENT_EXTENSIONS.iter().any(|ext| p.ends_with(ext))
}

/// Hand a document path to the frontend: queue it for `take_pending_file`
/// and ping the `document-open-file` event in case the webview is already
/// listening. (The event name is historical; it carries every format now.)
fn queue_pending(app: &tauri::AppHandle, path: String) {
    let path = path.trim().trim_matches('"').to_string();
    if !is_document_path(&path) {
        return;
    }
    if let Some(state) = app.try_state::<PendingFile>()
        && let Ok(mut guard) = state.0.lock() {
            *guard = Some(path.clone());
        }
    let _ = app.emit("document-open-file", path);
}

/// Returns the pending OS-opened document path (if any) and clears it. The
/// frontend pulls this once on mount and again whenever `document-open-file`
/// pings, so a file opened at launch survives the webview not being ready
/// and one opened mid-run never opens twice.
#[tauri::command]
fn take_pending_file(state: tauri::State<'_, PendingFile>) -> Option<String> {
    state.0.lock().ok().and_then(|mut g| g.take())
}

/// The gate the `read_file_*` commands apply before touching the filesystem.
/// They are exposed to the webview, which parses untrusted documents;
/// requiring an absolute path with a known document suffix keeps them from
/// being a general file-read primitive while every real open path (dialog,
/// drag-drop, OS "open with") already hands over exactly that.
fn ensure_readable_document(path: &str) -> Result<(), String> {
    let looks_absolute = path.starts_with('/')          // POSIX
        || path.starts_with("\\\\")                      // Windows UNC share
        || path.as_bytes().get(1) == Some(&b':');       // Windows drive letter
    if !looks_absolute {
        return Err(format!("refusing to read a non-absolute path: {path}"));
    }
    let lower = path.to_lowercase();
    if !DOCUMENT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err(format!("refusing to read a non-document file: {path}"));
    }
    Ok(())
}

/// Read a file's bytes for the webview. Returned as an IPC `Response` so the
/// JS side receives an `ArrayBuffer` (no JSON round-trip for megabytes).
/// Errors resolve as a rejected invoke, which the engine falls back from.
/// The read itself runs on the blocking pool so a 50 MB book does not stall
/// the async runtime while the bytes come off disk.
#[tauri::command]
async fn read_file_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    ensure_readable_document(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::read(&path)
            .map(tauri::ipc::Response::new)
            .map_err(|e| format!("could not read {path}: {e}"))
    })
    .await
    .map_err(|e| format!("read worker failed: {e}"))?
}

/// Read a text document for the webview as a UTF-8 string. The reflowable
/// formats (plain text, Markdown) are small enough that a JSON string is
/// the right shape — no ArrayBuffer plumbing needed. Undecodable bytes are
/// replaced rather than erroring: a reader that shows a mojibake box beats
/// one that refuses the file.
#[tauri::command]
async fn read_file_text(path: String) -> Result<String, String> {
    ensure_readable_document(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        std::fs::read(&path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|e| format!("could not read {path}: {e}"))
    })
    .await
    .map_err(|e| format!("read worker failed: {e}"))?
}

/// Show/hide the native macOS traffic lights with dynamic vertical centering.
///
/// `titleBarStyle: "Overlay"` lets the webview draw under the lights, but CSS
/// cannot hide or re-center the buttons themselves — they are native NSViews.
/// Delegates to `macos::traffic_light`, which owns the container geometry:
///
/// ```text
/// y = ((header_height - button_height)/2 + natural_origin_y).max(0)
/// container.height = visible ? button_height + y : 0
/// ```
///
/// `tauri.conf.json:trafficLightPosition {x:20,y:25}` remains only the
/// pre-mount fallback; after the first `invoke` Rust is the sole authority
/// for `y`. `header_height` comes from the frontend's `ResizeObserver` on
/// `#toolbar-row` (`h-12` = 48px). Pass `0` to keep the last height.
/// Caches `natural_origin_y` in `OnceLock` so Tahoe's ~7pt vs Sonoma's ~5pt
/// is self-correcting without drift. Re-applied on `ThemeChanged`.
#[tauri::command]
fn set_traffic_lights(window: tauri::Window, visible: bool, header_height: Option<f64>) {
    #[cfg(target_os = "macos")]
    {
        macos::traffic_light::set_traffic_lights(window, visible, header_height.unwrap_or(0.0));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible, header_height);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(macos::traffic_light::init())
        // A PDF double-clicked while the app is already running must land in
        // the EXISTING window, not open a second one (Windows/Linux).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(path) = argv.into_iter().find(|a| is_document_path(a)) {
                queue_pending(app, path);
            }
        }))
        .manage(PendingFile(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            take_pending_file,
            read_file_bytes,
            read_file_text,
            set_traffic_lights,
            commands::ai::explain_word
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("error while running tauri application: {e}");
            std::process::exit(1);
        });

    app.run(|app_handle, event| match event {
        // macOS: files opened with this app (Finder double-click, `open -a`,
        // LaunchServices) arrive here — at launch and while running. The
        // variant is cfg'd to Apple platforms in tauri itself.
        #[cfg(target_os = "macos")]
        RunEvent::Opened { urls } => {
            for url in urls {
                let path = url
                    .to_file_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| url.to_string());
                queue_pending(app_handle, path);
            }
        }
        // Windows/Linux: the initial launch's PDF path is a plain argv entry
        // (Explorer "Open with" / double-click under a file association).
        RunEvent::Ready => {
            for arg in std::env::args().skip(1) {
                if is_document_path(&arg) {
                    queue_pending(app_handle, arg);
                    break;
                }
            }
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::{ensure_readable_document, is_document_path};

    #[test]
    fn real_open_paths_pass_the_gate() {
        ensure_readable_document("/Users/thiha/Documents/paper.pdf").unwrap();
        ensure_readable_document("C:\\Users\\thiha\\Desktop\\report.PDF").unwrap();
        ensure_readable_document("\\\\NAS\\books\\scan.pdf").unwrap();
    }

    #[test]
    fn every_document_format_passes_the_gate() {
        ensure_readable_document("/Users/thiha/notes.txt").unwrap();
        ensure_readable_document("/Users/thiha/notes.TEXT").unwrap();
        ensure_readable_document("/Users/thiha/README.md").unwrap();
        ensure_readable_document("C:\\docs\\guide.markdown").unwrap();
        ensure_readable_document("/Users/thiha/draft.mdown").unwrap();
    }

    #[test]
    fn everything_that_is_not_a_document_is_refused() {
        // The exfiltration class: arbitrary readable files without a
        // document suffix.
        assert!(ensure_readable_document("/home/thiha/.ssh/id_rsa").is_err());
        assert!(ensure_readable_document("/etc/passwd").is_err());
        assert!(ensure_readable_document("/home/thiha/data.json").is_err());
        assert!(ensure_readable_document("").is_err());
    }

    #[test]
    fn relative_paths_are_refused() {
        assert!(ensure_readable_document("sample.pdf").is_err());
        assert!(ensure_readable_document("../notes.txt").is_err());
        assert!(ensure_readable_document("./report.md").is_err());
    }

    #[test]
    fn the_os_handoff_admits_every_format_and_only_those() {
        assert!(is_document_path("/books/dune.pdf"));
        assert!(is_document_path("\"C:\\My Docs\\notes.txt\""));
        assert!(is_document_path("/books/chapter.MD"));
        assert!(!is_document_path("/books/image.png"));
        assert!(!is_document_path("/books/notes.md.bak"));
    }
}
