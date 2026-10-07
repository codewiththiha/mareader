//! macOS window chrome helpers.

#[cfg(target_os = "macos")]
pub mod traffic_light;

#[cfg(not(target_os = "macos"))]
pub mod traffic_light {
    use tauri::plugin::{Builder, TauriPlugin};

    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light").build()
    }
    // No `set_traffic_lights` stub: it is only called under a macOS cfg.
}
