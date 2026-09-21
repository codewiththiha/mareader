//! The shelf cover image and its row-keyed map.
//!
//! Pure data on purpose: a cover is a baked data URL plus its aspect, and
//! every surface that shows one — the shelf cards, the rail's identity row —
//! needs exactly this shape and nothing about where it came from.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// A baked cover: the data URL and its natural size, so a card can reserve
/// the right box before the image decodes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// Behind an `Arc`: a cover is tens of kilobytes, and the map is read out of
/// a signal on every shelf render and cloned whole before every save.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

#[cfg(test)]
mod tests {
    use super::*;

    /// The cover round-trips through the persisted JSON shape: the shell
    /// writes these into the blob it stores, so a field rename here is a
    /// silent loss of every saved cover.
    #[test]
    fn cover_image_round_trips_camel_case() {
        let cover = CoverImage {
            data_url: "data:image/png;base64,AAA".to_string(),
            width: 120.0,
            height: 180.0,
        };
        let json = serde_json::to_string(&cover).unwrap();
        assert!(json.contains("\"dataUrl\""));
        assert!(json.contains("\"width\""));
        let back: CoverImage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cover);
    }
}
