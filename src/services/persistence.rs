//! The shell's persistence writes for what crosses the boundary.

use runtime_contract::boundary::ReadPoint;

pub fn apply_read_point(point: &ReadPoint) {
    storage::apply_read_point(point);
}

pub fn save_settings(settings: &reader_core::settings::Settings) {
    let _ = storage::save_settings(settings);
}

/// One document's marks, as the reader encoded them.
pub fn save_gloss(key: &str, marks: &str) {
    storage::persist_encoded_gloss(key, marks);
}

pub fn save_cover(path: &str, image: runtime_contract::covers::CoverImage) {
    // The same quota rule the library's import queue kept.
    let mut map = storage::load_covers();
    if !map.contains_key(path) && map.len() >= runtime_contract::covers::COVER_CAP {
        // Drop the oldest entry: the least recently covered file loses its art.
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
