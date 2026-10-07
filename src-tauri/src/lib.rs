//! Mareader's Tauri backend: the webview plus the two OS touch-points.

use std::sync::Mutex;

use tauri::{Emitter, Manager, RunEvent};

mod ai;
mod commands;
mod macos;

/// Every extension the reader opens; the filesystem gates accept exactly these.
const DOCUMENT_EXTENSIONS: &[&str] = &[".pdf", ".txt", ".text", ".md", ".markdown", ".mdown"];

/// The OS-opened document path the frontend has not collected yet.
struct PendingFile(Mutex<Option<String>>);

/// True for anything to open as a document; also strips Windows shell quoting.
fn is_document_path(raw: &str) -> bool {
    let p = raw.trim().trim_matches('"').to_lowercase();
    DOCUMENT_EXTENSIONS.iter().any(|ext| p.ends_with(ext))
}

/// Queue a document path for `take_pending_file` and ping the webview.
fn queue_pending(app: &tauri::AppHandle, path: String) {
    let path = path.trim().trim_matches('"').to_string();
    if !is_document_path(&path) {
        return;
    }
    if let Some(state) = app.try_state::<PendingFile>()
        && let Ok(mut guard) = state.0.lock()
    {
        *guard = Some(path.clone());
    }
    let _ = app.emit("document-open-file", path);
}

/// The pending OS-opened document path, cleared on read.
fn take_pending_file(state: tauri::State<'_, PendingFile>) -> Option<String> {
    state.0.lock().ok().and_then(|mut g| g.take())
}

/// Whether a path is absolute in any of the three hosts' spellings.
pub(crate) fn path_looks_absolute(path: &str) -> bool {
    path.starts_with('/')                  // POSIX
        || path.starts_with("\\\\")        // Windows UNC share
        || path.as_bytes().get(1) == Some(&b':') // Windows drive letter
}

/// The gate the file commands apply: absolute path, known document suffix.
pub(crate) fn ensure_readable_document(path: &str) -> Result<(), String> {
    if !path_looks_absolute(path) {
        return Err(format!("refusing to read a non-absolute path: {path}"));
    }
    let lower = path.to_lowercase();
    if !DOCUMENT_EXTENSIONS.iter().any(|ext| lower.ends_with(ext)) {
        return Err(format!("refusing to read a non-document file: {path}"));
    }
    Ok(())
}

/// Read a file's bytes as an IPC `Response`, on the blocking pool.
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

/// Read a text document as UTF-8; undecodable bytes are replaced, not refused.
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

/// Show or hide the native macOS traffic lights, centred on the header.
fn set_traffic_lights(window: tauri::Window, visible: bool, header_height: Option<f64>) {
    #[cfg(target_os = "macos")]
    {
        macos::traffic_light::set_traffic_lights(window, visible, header_height.unwrap_or(0.0));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window, visible, header_height);
}

/// The frontend's boot report, one stderr line per transition.
fn boot_report(report: String) {
    let line = report
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .chars()
        .take(160)
        .collect::<String>();
    if !line.is_empty() {
        eprintln!("[mareader] {line}");
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(macos::traffic_light::init())
        // A file double-clicked while running lands in the existing window.
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
            boot_report,
            commands::ai::explain_word,
            commands::library::scan_folder,
            commands::library::verify_paths,
            commands::library::store_books,
            commands::library::delete_stored,
            commands::library::relocate_stored,
            commands::library::reveal_in_folder
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("error while running tauri application: {e}");
            std::process::exit(1);
        });

    app.run(|app_handle, event| match event {
        // macOS: Finder double-clicks and `open -a` arrive here.
        RunEvent::Opened { urls } => {
            for url in urls {
                let path = url
                    .to_file_path()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| url.to_string());
                queue_pending(app_handle, path);
            }
        }
        // Windows/Linux: the launch's file path is a plain argv entry.
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
