//! The cover-art cache's shape: one decoded thumbnail per book id.
//!
//! Pure data, and deliberately so. The shelf renders covers, the reader's rail
//! shows the one for the book it is holding, and the storage layer writes the
//! whole map to `localStorage` — three consumers on two sides of the app's
//! crate split, none of which may have to reach into the other's state to name
//! a cover. What fills the cache (decoding, the LRU cap, the prune pass) is a
//! service in the app crate; what a cover IS belongs to the library's domain.

use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

/// One book's cover, as the data URL the shelf paints and the dimensions it
/// lays out with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// Covers by book id.
///
/// Behind an `Arc`: a cover is tens of kilobytes, and the map is read out of
/// a signal on every shelf render and cloned whole before every save.
pub type CoverMap = HashMap<String, Arc<CoverImage>>;
