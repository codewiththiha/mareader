//! The shell's persistence writes: the ONE authority for the durable keys.
//! Runtime sessions hand data across the boundary; this module touches
//! storage. The read-point recorder reuses the reader-side logic that used
//! to write the blob directly.

use runtime_contract::boundary::ReadPoint;

pub fn apply_read_point(point: &ReadPoint) {
    storage::apply_read_point(point);
}

pub fn save_settings(settings: &reader_core::settings::Settings) {
    let _ = storage::save_settings(settings);
}

pub fn save_library(blob: &library_core::blob::LibraryBlob) {
    let _ = storage::save_library(blob);
}

pub fn save_cover(path: &str, image: runtime_contract::covers::CoverImage) {
    // The same quota rule the library's import queue kept: the cap is
    // enforced by whoever writes, not by whoever happens to prune next.
    let mut map = storage::load_covers();
    if !map.contains_key(path) && map.len() >= runtime_contract::covers::COVER_CAP {
        // Drop the oldest entry (insertion-ordered map): the least recently
        // COVERED file loses its art, never its rows.
        if let Some(oldest) = map.keys().next().cloned() {
            map.remove(&oldest);
        }
    }
    map.insert(path.to_string(), std::sync::Arc::new(image));
    let _ = storage::save_covers(&map);
}

pub fn resolve_launch(path: &str) -> Option<runtime_contract::boundary::LaunchDocument> {
    storage::resolve_launch(path)
}
