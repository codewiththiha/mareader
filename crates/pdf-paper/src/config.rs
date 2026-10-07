//! Which pixels carry the blend backdrop's paper colour.
use serde::{Deserialize, Serialize};

/// Edge-strip bounds, in sampled-raster pixels.
const MIN_EDGE_WIDTH: u32 = 2;
const MAX_EDGE_WIDTH: u32 = 32;

/// The default edge-strip thickness: a thin slice of each side.
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

/// Every knob the paper pipeline exposes, in one value.
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
    /// Clamp every knob into its legal range.
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
        // An older blob carries retired keys; defaults fill the rest.
        let c: PaperConfig = serde_json::from_str(r#"{"mode":"fixed","scan_pages":100}"#).unwrap();
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
