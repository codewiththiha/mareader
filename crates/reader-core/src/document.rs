//! Status and page size shared by every format. These used to live next to
//! the PDF bridge, which meant a shelf that only needed the enum still linked
//! that bridge.

use serde::{Deserialize, Serialize};

/// Where an open stands. Idle is the shelf. Opening is the handoff. Ready is
/// a document the viewer can show. Error is a failed open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Idle,
    Opening,
    Ready,
    Error,
}

/// One page's intrinsic size, in PDF points or the reflow sheet's CSS pixels.
/// Field names are the wire names the open result already uses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}
