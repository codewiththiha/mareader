//! Scroll anchoring: keeping the reader's view pinned while the layout
//! changes underneath it (measurements landing, zoom rescales).
//!
//! These are pure functions over any [`Layout`] — the framework adapter
//! applies their results as a single "set scroll" write per frame.

use crate::layout::Layout;

/// What to hold visually fixed while sizes change.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AnchorPolicy {
    /// Keep `item` at its current screen position. Corrections apply only
    /// when the changed item is strictly above it. (The reader's page.)
    Item(usize),
    /// Keep the point `frac` (0..=1) of the way down `item` fixed. A resize
    /// of the anchor item itself shifts the scroll by `delta * frac`, so a
    /// point mid-item stays mid-item.
    Fractional {
        /// The anchor item.
        item: usize,
        /// Fraction into the item, clamped to `0..=1`.
        frac: f64,
    },
}

/// The corrected scroll position after a size change.
///
/// `changed` is the index that resized; `delta` is the signed change
/// (the return value of [`Layout::set_size`]). Returns `scroll_top`
/// unchanged when the change cannot move the anchor.
#[inline]
pub fn correct(scroll_top: f64, policy: AnchorPolicy, changed: usize, delta: f64) -> f64 {
    if delta == 0.0 {
        return scroll_top;
    }
    match policy {
        AnchorPolicy::Item(anchor) => {
            if changed < anchor {
                scroll_top + delta
            } else {
                scroll_top
            }
        }
        AnchorPolicy::Fractional { item, frac } => {
            if changed < item {
                scroll_top + delta
            } else if changed == item {
                scroll_top + delta * frac.clamp(0.0, 1.0)
            } else {
                scroll_top
            }
        }
    }
}

/// The content point to pin for a viewport-relative anchor: the item under
/// the point `frac` of the way down the viewport (0.5 = center), and that
/// point's offset from the item's top. Feed both into [`rescale_anchor`].
pub fn pin_at<L: Layout + ?Sized>(
    layout: &L,
    scroll_top: f64,
    viewport: f64,
    frac: f64,
) -> (usize, f64) {
    if layout.is_empty() {
        return (0, 0.0);
    }
    let target = scroll_top + viewport.max(0.0) * frac.clamp(0.0, 1.0);
    let item = layout.index_at(target).min(layout.item_count() - 1);
    (item, target - layout.offset(item))
}

/// New scroll position after multiplying **every** item size by `factor`,
/// such that the point `anchor_px` below the top of `anchor_item` stays at
/// the same viewport-relative position.
///
/// This is the zoom contract: the reader's eyes stay on the same content
/// point while the whole column rescales. Round-trips exactly (out then
/// back in returns the original scroll position) — see tests.
pub fn rescale_anchor<L: Layout + ?Sized>(
    layout: &L,
    scroll_top: f64,
    anchor_item: usize,
    anchor_px: f64,
    factor: f64,
) -> Option<f64> {
    if layout.is_empty() || factor <= 0.0 || factor.is_nan() {
        return None;
    }
    let item = anchor_item.min(layout.item_count() - 1);
    let size = layout.size(item).max(0.0);
    let clamped_anchor_px = anchor_px.clamp(0.0, size);
    let pin_old = layout.offset(item) + clamped_anchor_px;
    let on_screen = pin_old - scroll_top;

    // Only the item extents rescale; any gap between neighbouring items stays
    // whatever the layout currently reports. Rebuild the anchored item's new
    // offset from the old geometry instead of scaling the whole content point,
    // which would incorrectly scale fixed chrome gaps as well.
    let mut offset_new = 0.0;
    for index in 0..item {
        let size = layout.size(index).max(0.0);
        let next = layout.offset(index + 1);
        let gap_after = (next - layout.offset(index) - size).max(0.0);
        offset_new += size * factor + gap_after;
    }

    Some((offset_new + clamped_anchor_px * factor - on_screen).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{Layout, ListLayout};

    #[test]
    fn item_anchor_moves_only_for_changes_above() {
        let top = 1_000.0;
        assert_eq!(correct(top, AnchorPolicy::Item(10), 5, 50.0), 1_050.0);
        assert_eq!(correct(top, AnchorPolicy::Item(10), 10, 50.0), 1_000.0);
        assert_eq!(correct(top, AnchorPolicy::Item(10), 15, 50.0), 1_000.0);
        assert_eq!(correct(top, AnchorPolicy::Item(10), 5, 0.0), 1_000.0);
    }

    #[test]
    fn fractional_anchor_tracks_a_point_inside_the_item() {
        assert_eq!(
            correct(
                1_000.0,
                AnchorPolicy::Fractional {
                    item: 10,
                    frac: 0.3,
                },
                10,
                100.0,
            ),
            1_030.0
        );
        assert_eq!(
            correct(
                1_000.0,
                AnchorPolicy::Fractional {
                    item: 10,
                    frac: 0.3,
                },
                4,
                100.0,
            ),
            1_100.0
        );
        assert_eq!(
            correct(
                1_000.0,
                AnchorPolicy::Fractional {
                    item: 10,
                    frac: 0.3,
                },
                11,
                100.0,
            ),
            1_000.0
        );
    }

    #[test]
    fn pin_at_finds_the_item_under_the_viewport_center() {
        let l: ListLayout = ListLayout::uniform(10, 100.0, 0.0);
        let (item, px) = pin_at(&l, 0.0, 200.0, 0.5);
        assert_eq!(item, 1);
        assert!((px - 0.0).abs() < 1e-9);
    }

    #[test]
    fn rescale_round_trip_in_a_mixed_size_column() {
        let intrinsic: Vec<f64> = (0..300)
            .map(|i| match i {
                _ if i % 37 == 0 => 612.0,
                _ if i % 13 == 0 => 842.0,
                _ if i % 7 == 0 => 1008.0,
                _ => 792.0,
            })
            .collect();
        let vh = 800.0;
        let mut layout: ListLayout = ListLayout::new(intrinsic.iter().copied(), 24.0);

        let mut scroll = layout.offset(255) + layout.size(255) * 0.5 - vh * 0.5;
        let start = layout.dominant(scroll, vh);
        assert_eq!(start, 255, "test setup should start on item 255");

        let mut scale = 1.0_f64;
        for target in [0.5_f64, 0.25, 0.5, 1.0, 1.75, 1.0] {
            let factor = target / scale;
            let (item, px) = pin_at(&layout, scroll, vh, 0.5);
            scroll = rescale_anchor(&layout, scroll, item, px, factor)
                .expect("rescale should produce a position");
            scale = target;
            layout = ListLayout::new(intrinsic.iter().map(|h| h * scale), 24.0);
            assert_eq!(
                pin_at(&layout, scroll, vh, 0.5).0,
                start,
                "the anchor walked at scale {scale}"
            );
        }
        let home: ListLayout = ListLayout::new(intrinsic.iter().copied(), 24.0);
        let expect = home.offset(255) + home.size(255) * 0.5 - vh * 0.5;
        assert!(
            (scroll - expect).abs() < 0.01,
            "round trip drifted {scroll} vs {expect}"
        );
    }
}
