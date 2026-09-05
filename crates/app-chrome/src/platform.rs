//! Which desktop OS the webview is running on.
//!
//! The frontend is ONE wasm binary for all three desktops, so compile-time
//! `cfg` cannot tell it which window chrome exists: macOS owns native
//! traffic lights (hidden/shown and guttered by the app), while Windows and
//! Linux run frameless (`tauri.windows.conf.json` / `tauri.linux.conf.json`
//! strip the decorations) and get the app's own caption cluster. The user
//! agent settles it — each webview engine ships a stable platform token
//! (WKWebView "Macintosh", WebView2 "Windows NT", WebKitGTK "Linux").
//!
//! Probed once per process and parked in a `OnceLock`: the answer never
//! changes at runtime, so chrome reads it without re-entering JS, and host
//! `cargo test` (no webview at all) gets a truthful `Other`.

use std::sync::OnceLock;

/// The desktops this app ships on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DesktopPlatform {
    MacOs,
    Windows,
    Linux,
    /// A plain browser (`trunk serve`) or anything unrecognized.
    Other,
}

static PLATFORM: OnceLock<DesktopPlatform> = OnceLock::new();

/// The desktop this webview runs on.
fn platform() -> DesktopPlatform {
    *PLATFORM.get_or_init(detect)
}

fn detect() -> DesktopPlatform {
    if !cfg!(target_arch = "wasm32") {
        return DesktopPlatform::Other;
    }
    let Some(ua) = web_sys::window().map(|w| w.navigator().user_agent().unwrap_or_default()) else {
        return DesktopPlatform::Other;
    };
    // Order matters only in that "Macintosh" must win over the rest — the
    // tokens are mutually exclusive in practice (WebView2 never says Linux,
    // WebKitGTK never says Windows), so this is a partition, not a priority.
    if ua.contains("Macintosh") || ua.contains("Mac OS X") {
        DesktopPlatform::MacOs
    } else if ua.contains("Windows") {
        DesktopPlatform::Windows
    } else if ua.contains("Linux") || ua.contains("X11") || ua.contains("Wayland") {
        DesktopPlatform::Linux
    } else {
        DesktopPlatform::Other
    }
}

/// True where the native traffic lights exist — the app hides/shows them
/// and owes their corner a gutter. False everywhere else, Linux included:
/// frameless there means no server-side title bar at all.
pub fn is_macos() -> bool {
    platform() == DesktopPlatform::MacOs
}

/// True on Linux, where the frameless caption cluster draws GNOME-style
/// circular buttons instead of the Windows squares — the circles match
/// what a GNOME shell's header bar draws, so the window reads as native
/// to the desktop.
pub fn is_linux() -> bool {
    platform() == DesktopPlatform::Linux
}

/// True where the window is frameless and the app owes the user its own
/// caption buttons (minimize / maximize / close at the bar's far edge).
/// Also true in a plain browser on those hosts — the cluster renders (its
/// styling stays testable under `trunk serve`) and every call it can make
/// is a no-op there, exactly like every other Tauri surface.
pub fn uses_frameless_controls() -> bool {
    matches!(platform(), DesktopPlatform::Windows | DesktopPlatform::Linux)
}
