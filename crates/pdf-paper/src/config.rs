//! Which pixels of a page are trusted to carry the blend backdrop's paper
//! colour.
//!
//! The backdrop follows the reader: a colour PER PAGE, blended along the
//! scroll position so the backdrop arrives at the next page's paper at the
//! same moment the page itself does. The only choice left to the reader is
//! the detection area:
//!
//! * [`PaperArea::WholePage`] — every pixel of the sampled raster votes.
//! * [`PaperArea::Edges`] — only a thin strip along the page's left and right
//!   edges votes: the margins, where a scanned or decorated page still shows
//!   its honest paper even when the middle is full of artwork.
//!
//! Every knob is a plain field on [`PaperConfig`] — callers adjust the area
//! and the strip width without touching any other layer.

use serde::{Deserialize, Serialize};

/// Edge-strip bounds, in sampled-raster pixels (rasters are downscaled to a
/// ≤96px long edge before detection, so 10px is a real margin's worth).
/// Crate-internal: `sanitize` is the only consumer.
const MIN_EDGE_WIDTH: u32 = 2;
const MAX_EDGE_WIDTH: u32 = 32;

/// The default edge-strip thickness: a thin slice of each side, wide enough
/// that the margin's flat colour survives the downscale.
pub const DEFAULT_EDGE_WIDTH: u32 = 10;

/// Which pixels of a page raster carry the paper colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperArea {
    #[default]
    WholePage,
    Edges,
}

impl PaperArea {
    pub fn label(&self) -> &'static str {
        match self {
            Self::WholePage => "Whole Page",
            Self::Edges => "Edges",
        }
    }
}

/// Every knob the paper pipeline exposes, in one value: the detection area
/// and how thick the edge strips are when the area is [`PaperArea::Edges`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaperConfig {
    #[serde(default)]
    pub area: PaperArea,
    #[serde(default = "default_edge_width")]
    pub edge_width: u32,
}

fn default_edge_width() -> u32 {
    DEFAULT_EDGE_WIDTH
}

impl Default for PaperConfig {
    fn default() -> Self {
        Self {
            area: PaperArea::default(),
            edge_width: DEFAULT_EDGE_WIDTH,
        }
    }
}

impl PaperConfig {
    /// Clamp every knob into its legal range so a hand-edited settings blob
    /// (or a stale one written by an older build) can never configure a
    /// page-wide "edge".
    pub fn sanitize(&mut self) {
        self.edge_width = self.edge_width.clamp(MIN_EDGE_WIDTH, MAX_EDGE_WIDTH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_whole_page() {
        let c = PaperConfig::default();
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn a_config_round_trips_through_snake_case_json() {
        let c = PaperConfig {
            area: PaperArea::Edges,
            edge_width: 6,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""area":"edges""#), "{json}");
        assert_eq!(serde_json::from_str::<PaperConfig>(&json).unwrap(), c);
    }

    #[test]
    fn a_stale_blob_loads_and_fills_in_the_defaults() {
        // A blob written by an older build still carries the retired
        // `mode`/`scan_pages` keys; they are ignored and the defaults fill
        // in for everything the blob does not name.
        let c: PaperConfig =
            serde_json::from_str(r#"{"mode":"fixed","scan_pages":100}"#).unwrap();
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn sanitize_clamps_the_edge_width() {
        let mut c = PaperConfig {
            edge_width: 99,
            ..PaperConfig::default()
        };
        c.sanitize();
        assert_eq!(c.edge_width, MAX_EDGE_WIDTH);
        c.edge_width = 0;
        c.sanitize();
        assert_eq!(c.edge_width, MIN_EDGE_WIDTH);
    }
}
