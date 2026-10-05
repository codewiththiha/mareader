//! The format-neutral document status and intrinsic page geometry shared by
//! the workspace mirror and both pane runtimes. No engine is needed to name
//! a page or report an open.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageSize {
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocStatus {
    Idle,
    Opening,
    Ready,
    Error,
}
