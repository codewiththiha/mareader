//! macOS window chrome helpers.

#[cfg(target_os = "macos")]
pub mod traffic_light;

#[cfg(not(target_os = "macos"))]
pub mod traffic_light {
    use tauri::plugin::{Builder, TauriPlugin};
    #[allow(unused_imports)]
    pub fn init() -> TauriPlugin<tauri::Wry> {
        Builder::new("traffic_light").build()
    }
    pub fn set_traffic_lights(_window: tauri::Window, _visible: bool, _header_height: f64) {}
}
