//! The cover store's data types, named by both the library and //! `storage`.
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoverImage {
    pub data_url: String,
    pub width: f64,
    pub height: f64,
}

/// Behind an `Arc`: a cover is tens of kilobytes.
pub type CoverMap = std::collections::HashMap<String, Arc<CoverImage>>;

/// The cover queue's render width.
pub const COVER_WIDTH: f64 = 240.0;

/// The page aspect a tile assumes before a real cover arrives.
pub const DEFAULT_PAGE_ASPECT: f64 = 0.75;

/// The persisted map's entry cap (`library-core`'s blob budget rule).
pub const COVER_CAP: usize = 400;
