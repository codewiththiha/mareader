//! Visible-grid-row math: which thumbnail-grid rows overlap the viewport,
//! expanded by a buffer on each side.

//! Visible-window math: which grid rows overlap the viewport, and which
//! pages to keep mounted for the continuous render window.


pub fn visible_grid_rows(
    scroll_top: f64,
    viewport_h: f64,
    rows: usize,
    row_height: f64,
    buffer: usize,
) -> Option<(usize, usize)> {
    if rows == 0 || row_height <= 0.0 {
        return None;
    }
    let bottom = scroll_top + viewport_h.max(0.0);
    let grid_bottom = rows as f64 * row_height;
    // The viewport overlaps nothing: fully above or fully below the grid.
    if bottom < 0.0 || scroll_top >= grid_bottom {
        return None;
    }
    let mut first = (scroll_top / row_height).floor().max(0.0) as usize;
    let mut last = (bottom / row_height).floor().max(0.0) as usize;
    // Float safety: nudge the bottom boundary up a hair so a viewport ending
    // just short of a row boundary still renders that row.
    if bottom > scroll_top {
        last = ((bottom / row_height) + 1e-9).floor().max(0.0) as usize;
    }
    first = first.min(rows - 1);
    last = last.min(rows - 1);
    let first = first.saturating_sub(buffer);
    let last = (last + buffer).min(rows - 1);
    Some((first, last))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_grid_rows_windows() {
        // (scroll_top, viewport_h, rows, buffer, expected)
        let cases: &[(f64, f64, usize, usize, Option<(usize, usize)>)] = &[
            (0.0, 100.0, 3, 0, Some((0, 0))),
            (120.0, 200.0, 3, 0, Some((1, 2))),
            (0.0, 240.0, 3, 0, Some((0, 2))),
            (120.0, 200.0, 3, 1, Some((0, 2))),
            (240.0, 100.0, 3, 2, Some((0, 2))),
            // Exact boundaries: the row above has strictly scrolled out.
            (120.0, 100.0, 3, 0, Some((1, 1))),
            (240.0, 100.0, 3, 0, Some((2, 2))),
            // Zero-height viewport still resolves to the row under scroll_top.
            (50.0, 0.0, 3, 0, Some((0, 0))),
            (200.0, 0.0, 3, 0, Some((1, 1))),
            // No overlap: past the end, or entirely above the grid.
            (9999.0, 100.0, 3, 0, None),
            (-100.0, 50.0, 3, 0, None),
            // Single row, including its buffer clamp and its past-the-end case.
            (0.0, 100.0, 1, 0, Some((0, 0))),
            (50.0, 200.0, 1, 1, Some((0, 0))),
            (120.0, 100.0, 1, 0, None),
            // No rows at all.
            (0.0, 500.0, 0, 0, None),
            (0.0, 0.0, 0, 0, None),
        ];
        for &(st, vh, rows, buf, want) in cases {
            assert_eq!(
                visible_grid_rows(st, vh, rows, 120.0, buf),
                want,
                "st={st} vh={vh} rows={rows} buf={buf}"
            );
        }
    }
}
