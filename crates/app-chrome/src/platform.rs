//! Which desktop OS the webview runs on, probed from the user agent.

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
    // "Macintosh" must win over the rest: a partition, not a priority.
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

/// True where the native traffic lights exist.
pub fn is_macos() -> bool {
    platform() == DesktopPlatform::MacOs
}

/// True on Linux, where the captions draw GNOME-style circles.
pub fn is_linux() -> bool {
    platform() == DesktopPlatform::Linux
}

/// True where the window is frameless and the app owes it captions.
pub fn uses_frameless_controls() -> bool {
    matches!(
        platform(),
        DesktopPlatform::Windows | DesktopPlatform::Linux
    )
}
