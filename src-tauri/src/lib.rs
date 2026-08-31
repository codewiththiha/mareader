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
//!     webview mounted would otherwise be lost) and `pdf-open-file` is only
//!     the wake-up ping for files that arrive while it is already mounted.

use std::sync::Mutex;

use tauri::{Emitter, Manager, RunEvent};

mod ai;
mod commands;

/// The OS-opened PDF path the frontend has not collected yet.
struct PendingFile(Mutex<Option<String>>);

/// True for anything we should try to open as a document.
///
/// Also strips Windows shell quoting: `std::env::args()` on Windows does not
/// remove the quotes Explorer puts around `%1`, so `"C:\My Docs\a.pdf"`
/// arrives quoted and would fail the suffix test.
fn is_pdf_path(raw: &str) -> bool {
    let p = raw.trim().trim_matches('"');
    p.to_lowercase().ends_with(".pdf")
}

/// Hand a PDF path to the frontend: queue it for `take_pending_file` and
/// ping the `pdf-open-file` event in case the webview is already listening.
fn queue_pending(app: &tauri::AppHandle, path: String) {
    let path = path.trim().trim_matches('"').to_string();
    if !is_pdf_path(&path) {
        return;
    }
    if let Some(state) = app.try_state::<PendingFile>()
        && let Ok(mut guard) = state.0.lock() {
            *guard = Some(path.clone());
        }
    let _ = app.emit("pdf-open-file", path);
}

/// Returns the pending OS-opened PDF path (if any) and clears it. The
/// frontend pulls this once on mount and again whenever `pdf-open-file`
/// pings, so a file opened at launch survives the webview not being ready
/// and one opened mid-run never opens twice.
#[tauri::command]
fn take_pending_file(state: tauri::State<'_, PendingFile>) -> Option<String> {
    state.0.lock().ok().and_then(|mut g| g.take())
}

/// The gate `read_file_bytes` applies before touching the filesystem. The
/// command is exposed to the webview, which parses untrusted PDFs; requiring
/// an absolute path with a `.pdf` suffix keeps it from being a general
/// file-read primitive while every real open path (dialog, drag-drop, OS
/// "open with") already hands over exactly that.
fn ensure_readable_pdf(path: &str) -> Result<(), String> {
    let looks_absolute = path.starts_with('/')          // POSIX
        || path.starts_with("\\\\")                      // Windows UNC share
        || path.as_bytes().get(1) == Some(&b':');       // Windows drive letter
    if !looks_absolute {
        return Err(format!("refusing to read a non-absolute path: {path}"));
    }
    if !path.to_lowercase().ends_with(".pdf") {
        return Err(format!("refusing to read a non-PDF file: {path}"));
    }
    Ok(())
}

/// Read a file's bytes for the webview. Returned as an IPC `Response` so the
/// JS side receives an `ArrayBuffer` (no JSON round-trip for megabytes).
/// Errors resolve as a rejected invoke, which the engine falls back from.
#[tauri::command]
fn read_file_bytes(path: String) -> Result<tauri::ipc::Response, String> {
    ensure_readable_pdf(&path)?;
    std::fs::read(&path)
        .map(tauri::ipc::Response::new)
        .map_err(|e| format!("could not read {path}: {e}"))
}

/// Show/hide the native macOS traffic lights.
///
/// `titleBarStyle: "Overlay"` lets the webview draw under the lights, but CSS
/// cannot hide the buttons themselves — they are native NSViews owned by the
/// window. The three buttons share one container view (the superview of the
/// close button), so hiding that container hides all three. Driven by the
/// frontend's hover-reveal signal so the lights fade in/out with the titlebar.
///
/// macOS-only: on other platforms the window has a normal caption and this is
/// a no-op.
#[tauri::command]
fn set_traffic_lights(window: tauri::Window, visible: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::{NSView, NSWindowButton};
        // rwh 0.6: window_handle() -> WindowHandle::as_raw() gives the raw
        // handle. (HasRawWindowHandle::raw_window_handle() is the deprecated
        // path, and HasWindowHandle alone doesn't provide raw_window_handle.)
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};

        if let Ok(handle) = window.window_handle()
            && let RawWindowHandle::AppKit(h) = handle.as_raw()
        {
            // SAFETY: the handle lends us a pointer to the window's own
            // content view, which is alive for as long as the window is; we
            // only ever read through the borrow.
            let view = unsafe { &*h.ns_view.as_ptr().cast::<NSView>() };

            // NSWindowCloseButton; its superview hosts all three lights.
            if let Some(ns_window) = view.window()
                && let Some(button) = ns_window.standardWindowButton(NSWindowButton::CloseButton)
            {
                // SAFETY: `superview` hands back an unretained pointer, but
                // the container is owned by the window's view hierarchy and
                // outlives this command alongside the window itself.
                if let Some(container) = unsafe { button.superview() } {
                    // objc2 encodes BOOL per-architecture itself, so a plain
                    // bool is portable across Apple Silicon and Intel here.
                    container.setHidden(!visible);
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // A PDF double-clicked while the app is already running must land in
        // the EXISTING window, not open a second one (Windows/Linux).
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(path) = argv.into_iter().find(|a| is_pdf_path(a)) {
                queue_pending(app, path);
            }
        }))
        .manage(PendingFile(Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            take_pending_file,
            read_file_bytes,
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
                if is_pdf_path(&arg) {
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
    use super::ensure_readable_pdf;

    #[test]
    fn real_open_paths_pass_the_gate() {
        ensure_readable_pdf("/Users/thiha/Documents/paper.pdf").unwrap();
        ensure_readable_pdf("C:\\Users\\thiha\\Desktop\\report.PDF").unwrap();
        ensure_readable_pdf("\\\\NAS\\books\\scan.pdf").unwrap();
    }

    #[test]
    fn everything_that_is_not_a_pdf_is_refused() {
        // The exfiltration class: arbitrary readable files without a .pdf suffix.
        assert!(ensure_readable_pdf("/home/thiha/.ssh/id_rsa").is_err());
        assert!(ensure_readable_pdf("/etc/passwd").is_err());
        assert!(ensure_readable_pdf("").is_err());
    }

    #[test]
    fn relative_paths_are_refused() {
        assert!(ensure_readable_pdf("sample.pdf").is_err());
        assert!(ensure_readable_pdf("../notes.pdf").is_err());
        assert!(ensure_readable_pdf("./report.pdf").is_err());
    }
}
