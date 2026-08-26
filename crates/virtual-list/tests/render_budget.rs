#![allow(missing_docs)]

use virtual_list::{Budget, Layout, ListLayout, Viewport};

fn window(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    budget: Budget,
) -> Option<(usize, usize)> {
    ListLayout::<virtual_list::Strip>::new(heights.iter().copied(), gap)
        .window(scroll_top, Viewport::main_only(viewport_h), budget)
        .map(|w| (w.first, w.last))
}

fn span_overlapping(top: f64, height: f64, heights: &[f64], gap: f64) -> Option<(usize, usize)> {
    ListLayout::<virtual_list::Strip>::new(heights.iter().copied(), gap)
        .overlapping(top, height)
        .map(|w| (w.first, w.last))
}

const GAP: f64 = 24.0;

fn uniform(n: usize, h: f64) -> Vec<f64> {
    vec![h; n]
}

/// Park the viewport `frac` of the way down item `idx` (0-based).
fn scroll_into(idx: usize, frac: f64, heights: &[f64], vh: f64) -> f64 {
    let layout = ListLayout::<virtual_list::Strip>::new(heights.iter().copied(), GAP);
    layout.offset(idx) + heights[idx] * frac - vh * 0.5
}

#[test]
fn zoomed_in_mounts_fewer_items_than_zoomed_out() {
    let vh = 800.0;
    let budget = Budget::default();
    let count_at = |page_h: f64| {
        let h = uniform(40, page_h);
        let st = scroll_into(10, 0.5, &h, vh);
        let (f, l) = window(st, vh, &h, GAP, budget).unwrap();
        l - f + 1
    };
    let zoomed_out = count_at(396.0);
    let normal = count_at(792.0);
    let zoomed = count_at(2376.0);
    let max_zoom = count_at(3960.0);

    assert!(
        zoomed_out >= normal && normal > zoomed && zoomed >= max_zoom,
        "expected monotonic shrink, got {zoomed_out} / {normal} / {zoomed} / {max_zoom}"
    );
    assert_eq!(max_zoom, 1, "at max zoom only the item under the eyes");
    assert!(max_zoom < 7 && zoomed < 7);
}

#[test]
fn next_item_mounts_before_the_reader_arrives() {
    let vh = 800.0;
    let h = uniform(40, 3960.0);
    let budget = Budget::default();
    let idx = 10;

    let mid = scroll_into(idx, 0.5, &h, vh);
    assert_eq!(window(mid, vh, &h, GAP, budget), Some((idx, idx)));

    let layout = ListLayout::<virtual_list::Strip>::new(h.iter().copied(), GAP);
    let page_bottom = layout.offset(idx) + h[idx];
    let near = page_bottom - vh - 10.0;
    let (f, l) = window(near, vh, &h, GAP, budget).unwrap();
    assert!(
        l >= idx + 1,
        "next item should be mounted early, got {f}..={l}"
    );

    let (vf, vl) = span_overlapping(near, vh, &h, GAP).unwrap();
    assert_eq!((vf, vl), (idx, idx), "next item must not be on screen yet");
}

#[test]
fn item_above_is_dropped_once_far_enough_behind() {
    let vh = 800.0;
    let h = uniform(40, 3960.0);
    let budget = Budget::default();
    let idx = 10;
    let layout = ListLayout::<virtual_list::Strip>::new(h.iter().copied(), GAP);
    let top = layout.offset(idx);

    let just_in = top + 100.0;
    let (f, _) = window(just_in, vh, &h, GAP, budget).unwrap();
    assert_eq!(f, idx - 1, "the item just behind should still be warm");

    let deep = top + vh * 1.5;
    let (f2, _) = window(deep, vh, &h, GAP, budget).unwrap();
    assert_eq!(f2, idx, "the item behind should have been evicted");
}

#[test]
fn visible_items_are_never_evicted() {
    let cases = [
        (396.0, 800.0),
        (792.0, 800.0),
        (3960.0, 420.0),
        (200.0, 1200.0),
    ];
    for (page_h, vh) in cases {
        let h = uniform(30, page_h);
        for budget in [
            Budget::default(),
            Budget::screenfuls(0.0, 1),
            Budget::screenfuls(3.0, 2),
        ] {
            for step in 0..40 {
                let st = step as f64 * page_h * 0.37;
                let Some((f, l)) = window(st, vh, &h, GAP, budget) else {
                    continue;
                };
                if let Some((vf, vl)) = span_overlapping(st, vh, &h, GAP) {
                    assert!(
                        f <= vf && l >= vl,
                        "page_h={page_h} vh={vh} st={st} budget={budget:?}: \
                         mounted {f}..={l} does not cover visible {vf}..={vl}"
                    );
                }
            }
        }
    }
}

#[test]
fn never_exceeds_the_ceiling_unless_visibility_demands_it() {
    let vh = 800.0;
    let h = uniform(60, 120.0);
    for max_items in [1usize, 3, 5, 7, 12] {
        let budget = Budget::screenfuls(2.0, max_items);
        let st = 1000.0;
        let (f, l) = window(st, vh, &h, GAP, budget).unwrap();
        let n = l - f + 1;
        let visible_n = span_overlapping(st, vh, &h, GAP)
            .map(|(a, b)| b - a + 1)
            .unwrap_or(0);
        assert!(
            n <= max_items.max(visible_n),
            "max_items={max_items}: mounted {n} items (visible {visible_n})"
        );
    }
}

#[test]
fn degenerate_inputs() {
    let budget = Budget::default();
    assert_eq!(window(0.0, 800.0, &[], GAP, budget), None);

    let h = uniform(5, 500.0);
    assert!(window(1100.0, 0.0, &h, GAP, budget).is_some());
    assert_eq!(window(99_999.0, 800.0, &h, GAP, budget), None);
    assert_eq!(span_overlapping(99_999.0, 800.0, &h, GAP), None);

    let bad = Budget::screenfuls(0.0, 0);
    let (f, l) = window(1100.0, 800.0, &h, GAP, bad).unwrap();
    assert!(l >= f);
}

#[test]
fn gap_parking_still_mounts_neighbours() {
    let h = [100.0, 200.0, 100.0];
    let budget = Budget::screenfuls(1.0, 7);
    let got = window(104.0, 15.0, &h, GAP, budget);
    assert!(got.is_some(), "a gap position must still mount something");
    let (f, l) = got.unwrap();
    assert!(
        f == 0 && l >= 1,
        "expected the items either side, got {f}..={l}"
    );
}
