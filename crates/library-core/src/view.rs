//! How the library is looking at the moment: the layout, the cover treatment,
//! the column count and the sort.
//!
//! Deliberately NOT part of `reader_core::settings::Settings`: every write
//! there re-runs the theme projection and re-serialises the whole settings
//! JSON, which is why `src/state/library.rs` keeps the library out of it.

use serde::{Deserialize, Serialize};

use crate::sort::SortKey;

/// The narrowest and widest column counts the view menu offers.
pub const COLUMNS_MIN: u8 = 2;
pub const COLUMNS_MAX: u8 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryLayout {
    #[default]
    Grid,
    List,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CoverFit {
    #[default]
    Fit,
    /// Every cover cropped to A4 portrait.
    Crop,
}

impl CoverFit {
    pub fn label(self) -> &'static str {
        match self {
            CoverFit::Fit => "Fit",
            CoverFit::Crop => "Crop",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    #[serde(default)]
    pub layout: LibraryLayout,
    /// `None` is Auto (as many as fit). Ignored in the list layout, where the count is kept for the next visit to the grid.
    #[serde(default)]
    pub columns: Option<u8>,
    /// The count the auto flow currently produces, so the menu can show it live and the stepper's first press can pin it.
    #[serde(default = "default_auto_fit")]
    pub auto_fit: u8,
    #[serde(default)]
    pub cover: CoverFit,
    #[serde(default)]
    pub sort: SortKey,
    #[serde(default = "default_asc")]
    pub sort_asc: bool,
}

fn default_asc() -> bool {
    true
}

/// A mid-range guess, for a blob written before the grid reported the flow's count.
fn default_auto_fit() -> u8 {
    5
}

impl Default for LibraryView {
    fn default() -> Self {
        Self {
            layout: LibraryLayout::default(),
            columns: None,
            auto_fit: default_auto_fit(),
            cover: CoverFit::default(),
            sort: SortKey::default(),
            sort_asc: true,
        }
    }
}

impl LibraryView {
    /// The value the grid's `--lib-cols` property takes, spelled once so the CSS and the control cannot drift.
    pub fn columns_token(&self) -> String {
        match self.columns {
            Some(n) => n.to_string(),
            None => "auto-fill".to_string(),
        }
    }

    /// Auto is a real target: the first press pins the count the flow is showing, so only the list layout kills the stepper.
    pub fn columns_enabled(&self) -> bool {
        !self.is_list()
    }

    /// From Auto the first press pins what the flow was showing and steps from that.
    /// A no-op in the list, so a control rendered disabled cannot be driven by a stray key.
    pub fn step_columns(&mut self, delta: i32) {
        if self.is_list() {
            return;
        }
        let base = u16::from(self.columns.unwrap_or(self.auto_fit));
        let next =
            (i32::from(base) + delta).clamp(i32::from(COLUMNS_MIN), i32::from(COLUMNS_MAX));
        self.columns = Some(next as u8);
    }

    /// One spelling, read by [`report_auto_fit`](Self::report_auto_fit) and by the grid that decides whether a report is worth a write.
    pub fn clamped_fit(fit: u8) -> u8 {
        fit.clamp(COLUMNS_MIN, COLUMNS_MAX)
    }

    /// The only writer of `auto_fit`; `columns` is the reader's pin and a measurement must not move it.
    pub fn report_auto_fit(&mut self, fit: u8) {
        self.auto_fit = Self::clamped_fit(fit);
    }

    pub fn auto_columns(&mut self) {
        self.columns = None;
    }

    /// A sorted shelf re-sorts on the next render, so a drop would be undone before the reader saw it land.
    pub fn drag_reorders(&self) -> bool {
        self.sort.is_manual()
    }

    pub fn is_list(&self) -> bool {
        self.layout == LibraryLayout::List
    }
}

/// Clamp a persisted view into the range the menu offers. Idempotent.
pub fn sanitize(view: &mut LibraryView) {
    view.columns = view.columns.map(|n| n.clamp(COLUMNS_MIN, COLUMNS_MAX));
    view.auto_fit = view.auto_fit.clamp(COLUMNS_MIN, COLUMNS_MAX);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_the_look_the_library_has_today() {
        let v = LibraryView::default();
        assert_eq!(v.layout, LibraryLayout::Grid);
        assert_eq!(v.columns, None, "Auto until the reader says otherwise");
        assert_eq!(v.auto_fit, 5, "a mid-range guess until the grid reports");
        assert_eq!(v.cover, CoverFit::Fit);
        assert_eq!(v.sort, SortKey::Manual);
        assert!(v.sort_asc);
        assert_eq!(v.columns_token(), "auto-fill");
        assert!(
            v.columns_enabled(),
            "Auto is a count the stepper can step from"
        );
        assert!(v.drag_reorders(), "a manual grid is the one a drag writes to");
    }

    #[test]
    fn a_blob_without_a_view_loads_the_default() {
        let v: LibraryView = serde_json::from_str("{}").unwrap();
        assert_eq!(v, LibraryView::default());
    }

    #[test]
    fn a_blob_from_before_the_grid_reported_loads_a_mid_range_auto_fit() {
        let v: LibraryView = serde_json::from_str(r#"{"columns":null}"#).unwrap();
        assert_eq!(v.auto_fit, 5);
        assert_eq!(v.columns, None);
    }

    #[test]
    fn the_view_persists_under_its_camel_case_names() {
        let v = LibraryView {
            layout: LibraryLayout::List,
            columns: Some(4),
            auto_fit: 7,
            cover: CoverFit::Crop,
            sort: SortKey::LastRead,
            sort_asc: false,
        };
        let json = serde_json::to_string(&v).unwrap();
        assert!(json.contains("\"layout\":\"list\""), "{json}");
        assert!(json.contains("\"autoFit\":7"), "{json}");
        assert!(json.contains("\"cover\":\"crop\""), "{json}");
        assert!(json.contains("\"sort\":\"lastRead\""), "{json}");
        let back: LibraryView = serde_json::from_str(&json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn a_pinned_count_steps_inside_its_range() {
        let mut v = LibraryView::default();
        v.columns = Some(6);
        assert_eq!(v.columns_token(), "6");
        assert!(v.columns_enabled());
        v.step_columns(1);
        assert_eq!(v.columns, Some(7));
        v.step_columns(-20);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        v.step_columns(99);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
        v.auto_columns();
        assert_eq!(v.columns, None);
        assert_eq!(v.columns_token(), "auto-fill");
    }

    #[test]
    fn a_report_never_pins_the_count_it_reports() {
        // The grid measures the flow while Auto owns the layout. A report that
        // pinned would freeze the count on the first resize after a launch, and
        // the reader's Auto would silently stop being auto.
        let mut v = LibraryView::default();
        v.report_auto_fit(6);
        assert_eq!(v.auto_fit, 6);
        assert_eq!(v.columns, None, "Auto still owns the layout");
        assert_eq!(v.columns_token(), "auto-fill");
        v.report_auto_fit(4);
        assert_eq!(v.auto_fit, 4, "and it keeps following the window");
        assert_eq!(v.columns, None);
    }

    #[test]
    fn stepping_from_auto_pins_what_auto_was_showing() {
        let mut v = LibraryView::default();
        v.auto_fit = 7;
        assert_eq!(v.columns, None, "Auto still owns the layout");
        v.step_columns(1);
        assert_eq!(
            v.columns,
            Some(8),
            "the first + pins what Auto was showing and steps from there"
        );
        assert_eq!(v.columns_token(), "8");
        v.step_columns(-2);
        assert_eq!(v.columns, Some(6), "and from then on it steps the pin");
        v.step_columns(-99);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        v.step_columns(99);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
    }

    #[test]
    fn the_stepper_is_live_in_grid_and_dead_in_list() {
        let mut v = LibraryView::default();
        assert!(v.columns_enabled(), "live in a grid under Auto…");
        v.columns = Some(4);
        assert!(v.columns_enabled(), "…and with a pinned count");
        v.layout = LibraryLayout::List;
        assert!(!v.columns_enabled(), "dead only where there are no columns");
        v.step_columns(1);
        assert_eq!(v.columns, Some(4), "and a stray key cannot drive it");
        v.layout = LibraryLayout::Grid;
        assert!(
            v.columns_enabled(),
            "the pin survived the list, so the grid returns as it left"
        );
    }

    #[test]
    fn a_report_outside_the_range_is_clamped_not_trusted() {
        let mut v = LibraryView::default();
        v.report_auto_fit(0);
        assert_eq!(v.auto_fit, COLUMNS_MIN);
        v.report_auto_fit(200);
        assert_eq!(v.auto_fit, COLUMNS_MAX);
        assert_eq!(LibraryView::clamped_fit(0), COLUMNS_MIN);
        assert_eq!(LibraryView::clamped_fit(200), COLUMNS_MAX);
        assert_eq!(LibraryView::clamped_fit(5), 5);
    }

    #[test]
    fn a_report_refreshes_auto_without_overriding_a_pin() {
        let mut v = LibraryView {
            columns: Some(4),
            ..LibraryView::default()
        };
        v.report_auto_fit(9);
        assert_eq!(v.auto_fit, 9, "the flow's count is recorded…");
        assert_eq!(v.columns, Some(4), "…and the reader's pin is left alone");
    }

    #[test]
    fn the_list_layout_has_no_columns_to_step() {
        let mut v = LibraryView {
            layout: LibraryLayout::List,
            columns: Some(5),
            ..LibraryView::default()
        };
        sanitize(&mut v);
        assert_eq!(
            v.columns,
            Some(5),
            "the count survives the list, to return with the grid"
        );
        assert!(!v.columns_enabled());
        assert!(v.is_list());
        v.step_columns(1);
        assert_eq!(v.columns, Some(5), "a list ignores a step");
    }

    #[test]
    fn a_stored_column_count_is_clamped_on_load() {
        let mut v: LibraryView = serde_json::from_str(r#"{"columns":99}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.columns, Some(COLUMNS_MAX));
        let mut v: LibraryView = serde_json::from_str(r#"{"columns":0}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.columns, Some(COLUMNS_MIN));
        let mut v: LibraryView = serde_json::from_str(r#"{"autoFit":99}"#).unwrap();
        sanitize(&mut v);
        assert_eq!(v.auto_fit, COLUMNS_MAX);
    }

    #[test]
    fn sorting_stands_a_drag_down() {
        let mut v = LibraryView::default();
        assert!(v.drag_reorders());
        v.sort = SortKey::Title;
        assert!(!v.drag_reorders());
        v.sort = SortKey::Manual;
        assert!(v.drag_reorders());
    }

    #[test]
    fn every_cover_and_layout_has_a_label() {
        assert_eq!(CoverFit::Fit.label(), "Fit");
        assert_eq!(CoverFit::Crop.label(), "Crop");
    }
}
