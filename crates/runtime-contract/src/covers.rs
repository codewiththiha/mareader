//! The cover store's data types. They live here — not beside the library
//! state — because both the library runtime (which builds the map) and the
//! storage crate (which loads and saves it) name them, and the storage crate
//! must not depend on the library runtime.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

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

/// The cover queue's render width: one number for both renders of the same
/// art, so the cache the open files into is the cache the queue filled.
pub const COVER_WIDTH: f64 = 240.0;

/// The page aspect (height/width) a tile assumes before a real cover or a
/// page size arrives: the shelf's placeholder art and the reader's missing-
/// size fallback must draw the SAME box or the grid would jump when the real
/// cover lands. Both runtimes name that one number.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// The persisted map's entry cap (`library-core`'s blob budget rule).
pub const COVER_CAP: usize = 400;
