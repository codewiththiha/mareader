//! The reader's layout policy: what the chrome shows, how pages are framed,
//! and how the paper blends into the backdrop.
//!
//! Split out of the settings barrel so the Layout tab's schema is one file;
//! the persisted field names are the storage contract and did not move.

use serde::{Deserialize, Serialize};

use crate::zoom_math::FitMode;

use super::on_true;

fn default_page_margin() -> f64 { 0.0 }
fn default_label_max_pct() -> f64 { 100.0 }
/// The fit mode a document opens with. `FitMode::None` is not a startup mode,
/// which is why [`super::sanitize`] retries it.
pub(super) fn default_startup_fit() -> FitMode { FitMode::Page }

/// How the appearance reaches the pixels of a page.
///
/// `Live` leaves the raw raster on screen and lets the compositor apply the
/// filter and blend every frame, so a page and the document backdrop go
/// through ONE floating-point pass — the two match exactly, which is what
/// makes blend mode look seamless. `Baked` burns the same pipeline into each
/// raster once per appearance change; the compositor then draws plain opaque
/// textures, which is cheaper per frame but re-quantizes in integer stages,
/// so a baked page can never be bit-identical to the live composite.
///
/// The reader picks: live for fidelity, baked for a lighter compositor on
/// large pages and slow machines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderPipeline {
    #[default]
    Live,
    Baked,
}

impl RenderPipeline {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Live => "Live",
            Self::Baked => "Baked",
        }
    }

    pub fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PageIndicatorStyle {
    #[default]
    PageNumber,
    Percentage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FloatingLabelStyle {
    #[default]
    #[serde(alias = "title")] // migration: old saved "title" loads as FileName
    FileName,
    Chapter,
}

/// Which pixels of a page raster the blend backdrop's paper pipeline trusts.
/// The `pdf-paper` crate owns the type; the barrel re-exports it so the
/// settings model stays the one place the reader's persisted knobs live.
use pdf_paper::PaperArea;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LayoutSettings {
    #[serde(default = "on_true")]
    pub page_indicator: bool,
    #[serde(default)]
    pub page_indicator_style: PageIndicatorStyle,
    #[serde(default = "on_true")]
    pub floating_label: bool,
    #[serde(default)]
    pub floating_label_style: FloatingLabelStyle,
    #[serde(default = "on_true")]
    pub progress_bar: bool,
    /// The fit mode applied when a document opens (or a book resumes): how the
    /// reader sizes the first page. `FitMode::Page` fits the whole page; `Width`
    /// fits the width. `None` is not a valid startup mode.
    #[serde(default = "default_startup_fit")]
    pub default_fit: FitMode,
    /// Remove the vertical gap between pages in scroll view.
    #[serde(default)]
    pub no_gap: bool,
    #[serde(default = "on_true")]
    pub auto_scale: bool,
    /// Let a page turn change the scale. On, arriving at a page whose size
    /// differs re-resolves the zoom: an active fit re-fits, and a hand-picked
    /// zoom is held to the new page's fit-width ceiling. Off, a page turn
    /// touches nothing — the zoom, the scroll position and the measured column
    /// all stay where the reader put them, and a page wider than the window
    /// overflows and scrolls. The WINDOW still re-fits either way; this
    /// switch is only about the page under the eyes.
    #[serde(default = "on_true")]
    pub auto_resize: bool,
    #[serde(default = "on_true")]
    pub page_shadow: bool,
    #[serde(default)]
    pub sidebar_overlay: bool,
    /// Paint the reader background with the page's own paper colour, a
    /// colour per page blended along the scroll. (Older blobs also carry
    /// `blend_scope` and `blend_scan_pages` from the retired fixed mode;
    /// serde ignores them.)
    #[serde(default)]
    pub blend_mode: bool,
    /// Which pixels of a page raster the detector trusts: the whole page,
    /// or just the thin left/right edge margins where artwork-heavy pages
    /// still show honest paper.
    #[serde(default)]
    pub blend_area: PaperArea,
    /// Horizontal inset around pages (CSS px). `0` removes the margin entirely.
    #[serde(default = "default_page_margin")]
    pub page_margin: f64,
    /// Keep the floating label on screen even when the sidebar or title bar
    /// would normally hide it, and ignore the width budget.
    #[serde(default)]
    pub floating_label_persist: bool,
    /// Share of the measured width budget (in percent) the floating label may
    /// consume before it fades out. `100` hides it only on a true overflow.
    #[serde(default = "default_label_max_pct")]
    pub floating_label_max_pct: f64,
}

impl Default for LayoutSettings {
    fn default() -> Self {
        Self {
            page_indicator: true,
            page_indicator_style: PageIndicatorStyle::PageNumber,
            floating_label: true,
            floating_label_style: FloatingLabelStyle::FileName,
            progress_bar: true,
            default_fit: default_startup_fit(),
            no_gap: false,
            auto_scale: true,
            auto_resize: true,
            page_shadow: true,
            sidebar_overlay: false,
            blend_mode: false,
            blend_area: PaperArea::default(),
            page_margin: default_page_margin(),
            floating_label_persist: false,
            floating_label_max_pct: default_label_max_pct(),
        }
    }
}
