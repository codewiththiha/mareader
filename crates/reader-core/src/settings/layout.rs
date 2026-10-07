//! The reader's layout policy: chrome, framing and the paper backdrop.

use serde::{Deserialize, Serialize};

use crate::zoom_math::FitMode;

use super::on_true;

fn default_page_margin() -> f64 {
    0.0
}
fn default_label_max_pct() -> f64 {
    100.0
}
/// The fit mode a document opens with; `None` is not valid.
pub(super) fn default_startup_fit() -> FitMode {
    FitMode::Width
}
/// The column-width dial's resting point: the natural column.
pub const DEFAULT_COLUMN_WIDTH_PCT: f64 = 100.0;
/// The column-width dial's floor, kept beside the setting.
pub const MIN_COLUMN_WIDTH_PCT: f64 = 60.0;
/// The column-width dial's ceiling. See [`MIN_COLUMN_WIDTH_PCT`].
pub const MAX_COLUMN_WIDTH_PCT: f64 = 140.0;

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

/// Which pixels the backdrop's paper pipeline trusts.
use pdf_paper::PaperArea;

/// The persisted knob's default: the natural column.
fn default_column_width_pct() -> f64 {
    DEFAULT_COLUMN_WIDTH_PCT
}

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
    /// The fit mode applied when a document opens or resumes.
    #[serde(default = "default_startup_fit")]
    pub default_fit: FitMode,
    /// Remove the vertical gap between pages in scroll view.
    #[serde(default)]
    pub no_gap: bool,
    #[serde(default = "on_true")]
    pub auto_scale: bool,
    /// Let a page turn change the scale.
    #[serde(default = "on_true")]
    pub auto_resize: bool,
    #[serde(default = "on_true")]
    pub page_shadow: bool,
    #[serde(default)]
    pub sidebar_overlay: bool,
    /// Paint the background with the page's own paper colour.
    #[serde(default)]
    pub blend_mode: bool,
    /// Which pixels the detector trusts: whole page, or margins.
    #[serde(default)]
    pub blend_area: PaperArea,
    /// Horizontal inset around pages (CSS px). `0` removes the margin entirely.
    #[serde(default = "default_page_margin")]
    pub page_margin: f64,
    /// Scale of the reading column, as percent of its natural width.
    #[serde(default = "default_column_width_pct")]
    pub column_width_pct: f64,
    /// Keep the floating label on screen, ignoring the width budget.
    #[serde(default)]
    pub floating_label_persist: bool,
    /// Share of the width budget the floating label may consume.
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
            column_width_pct: default_column_width_pct(),
            floating_label_persist: false,
            floating_label_max_pct: default_label_max_pct(),
        }
    }
}
