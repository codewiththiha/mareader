//! macOS window chrome helpers.

#[cfg(target_os = "macos")]
pub mod traffic_light;

#[cfg(not(target_os = "macos"))]
pub mod traffic_light {
    use tauri::plugin::{Builder, TauriPlugin};

    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light").build()
    }
    // No `set_traffic_lights` here on purpose: the command's body calls it
    // under `cfg(target_os = "macos")` only, so a stub on the other platforms
    // is dead code — and the Linux release build denies warnings.
}
