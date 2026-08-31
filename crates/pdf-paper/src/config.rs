//! Where the blend backdrop's paper colour comes from, and which pixels of a
//! page are trusted to carry it.
//!
//! Two modes, two areas — deliberately small vocabularies:
//!
//! * [`PaperMode::Fixed`] — one colour stands for the whole book: the pooled
//!   dominant colour of up to `scan_pages` pages (100 by default), persisted
//!   per document path so a reopened book repaints with zero sampling work.
//! * [`PaperMode::Continuous`] — a colour PER PAGE, blended along the reader's
//!   scroll position so the backdrop arrives at the next page's paper at the
//!   same moment the page itself does.
//!
//! * [`PaperArea::WholePage`] — every pixel of the sampled raster votes.
//! * [`PaperArea::Edges`] — only a thin strip along the page's left and right
//!   edges votes: the margins, where a scanned or decorated page still shows
//!   its honest paper even when the middle is full of artwork.
//!
//! The mode and area are orthogonal: either area feeds either mode, the scan
//! cap applies to the fixed scan, and every knob is a plain field on
//! [`PaperConfig`] — callers adjust pages, strips and mode without touching
//! any other layer.

use serde::{Deserialize, Serialize};

/// How many pages the fixed-mode scan samples by default. A cap, not a
/// target: shorter books stop early, and the number is adjustable through
/// [`PaperConfig::scan_pages`].
pub const DEFAULT_SCAN_PAGES: u32 = 100;

/// Scan-page bounds. The low end keeps "one page" expressible; the high end
/// stops a fat finger from asking for a thousand-page background scan.
pub const MIN_SCAN_PAGES: u32 = 1;
pub const MAX_SCAN_PAGES: u32 = 1000;

/// Edge-strip bounds, in sampled-raster pixels (rasters are downscaled to a
/// ≤96px long edge before detection, so 10px is a real margin's worth).
/// Crate-internal: `sanitize` is the only consumer.
const MIN_EDGE_WIDTH: u32 = 2;
const MAX_EDGE_WIDTH: u32 = 32;

/// The default edge-strip thickness: a thin slice of each side, wide enough
/// that the margin's flat colour survives the downscale.
pub const DEFAULT_EDGE_WIDTH: u32 = 10;

/// Where the blend backdrop's colour comes from.
///
/// The serde aliases migrate the pre-crate scopes in place: `"single"` (the
/// first rendered page stood in for the book) and `"document"` (the pooled
/// scan) both fold into `Fixed`, which is their union — the pooled scan with
/// the single page's colour as its interim.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PaperMode {
    #[serde(alias = "single", alias = "document")]
    #[default]
    Fixed,
    Continuous,
}

impl PaperMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Fixed => "Fixed",
            Self::Continuous => "Continuous",
        }
    }
}

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

    /// The area id the engine's `setPaper`/`persistPaper` calls expect.
    pub fn engine_id(&self) -> &'static str {
        match self {
            Self::WholePage => "whole",
            Self::Edges => "edges",
        }
    }
}

/// Every knob the paper pipeline exposes, in one value: the mode, the
/// detection area, how many pages a fixed scan may sample (100 by default)
/// and how thick the edge strips are when the area is [`PaperArea::Edges`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PaperConfig {
    #[serde(default)]
    pub mode: PaperMode,
    #[serde(default)]
    pub area: PaperArea,
    #[serde(default = "default_scan_pages")]
    pub scan_pages: u32,
    #[serde(default = "default_edge_width")]
    pub edge_width: u32,
}

fn default_scan_pages() -> u32 {
    DEFAULT_SCAN_PAGES
}

fn default_edge_width() -> u32 {
    DEFAULT_EDGE_WIDTH
}

impl Default for PaperConfig {
    fn default() -> Self {
        Self {
            mode: PaperMode::default(),
            area: PaperArea::default(),
            scan_pages: DEFAULT_SCAN_PAGES,
            edge_width: DEFAULT_EDGE_WIDTH,
        }
    }
}

impl PaperConfig {
    /// Clamp every knob into its legal range so a hand-edited settings blob
    /// (or a stale one written by an older build) can never configure an
    /// empty scan or a page-wide "edge".
    pub fn sanitize(&mut self) {
        self.scan_pages = self.scan_pages.clamp(MIN_SCAN_PAGES, MAX_SCAN_PAGES);
        self.edge_width = self.edge_width.clamp(MIN_EDGE_WIDTH, MAX_EDGE_WIDTH);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_fixed_whole_page_with_a_100_page_scan() {
        let c = PaperConfig::default();
        assert_eq!(c.mode, PaperMode::Fixed);
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.scan_pages, 100);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn the_legacy_scopes_load_as_fixed() {
        // "single" and "document" are the pre-crate scopes; both are the
        // fixed mode now, and old settings blobs must migrate silently.
        for old in ["single", "document"] {
            let c: PaperConfig =
                serde_json::from_str(&format!(r#"{{"mode":"{old}"}}"#)).unwrap();
            assert_eq!(c.mode, PaperMode::Fixed, "{old}");
        }
        let c: PaperConfig = serde_json::from_str(r#"{"mode":"continuous"}"#).unwrap();
        assert_eq!(c.mode, PaperMode::Continuous);
    }

    #[test]
    fn a_config_round_trips_through_snake_case_json() {
        let c = PaperConfig {
            mode: PaperMode::Continuous,
            area: PaperArea::Edges,
            scan_pages: 250,
            edge_width: 6,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains(r#""mode":"continuous""#), "{json}");
        assert!(json.contains(r#""area":"edges""#), "{json}");
        assert!(json.contains(r#""scan_pages":250"#), "{json}");
        assert_eq!(serde_json::from_str::<PaperConfig>(&json).unwrap(), c);
    }

    #[test]
    fn a_partial_blob_fills_in_the_defaults() {
        // A pre-area settings blob has no `area`/`scan_pages`/`edge_width`
        // keys: the 100-page default scan and whole-page area fill in.
        let c: PaperConfig = serde_json::from_str(r#"{"mode":"fixed"}"#).unwrap();
        assert_eq!(c.area, PaperArea::WholePage);
        assert_eq!(c.scan_pages, 100);
        assert_eq!(c.edge_width, DEFAULT_EDGE_WIDTH);
    }

    #[test]
    fn sanitize_clamps_the_knobs() {
        let mut c = PaperConfig {
            scan_pages: 0,
            edge_width: 99,
            ..PaperConfig::default()
        };
        c.sanitize();
        assert_eq!(c.scan_pages, MIN_SCAN_PAGES);
        assert_eq!(c.edge_width, MAX_EDGE_WIDTH);

        c.scan_pages = 10_000;
        c.sanitize();
        assert_eq!(c.scan_pages, MAX_SCAN_PAGES);
    }
}
