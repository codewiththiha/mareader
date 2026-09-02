//! Window chrome: the commands the frameless caption cluster fires, the
//! cluster itself (Windows squares, GNOME circles), and the macOS native
//! traffic lights.
//!
//! Nothing here knows what a document format is — the window does not care
//! what the app reads.

pub mod api;
pub mod caption;
pub mod caption_gnome;
pub mod caption_windows;
pub mod traffic_lights;

pub use caption::WindowControls;
