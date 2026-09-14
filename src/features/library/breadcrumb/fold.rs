//! The fold's arithmetic: how many crumbs the bar keeps, which of them the width hides, and how
//! the panel packs what is hidden into rows. Pure, so the fold's shape is host-tested rather than
//! eyeballed.

/// Three, because the bar's left cluster shares a row with a search box that has to stay usable and a window that can be 640px wide. Once the probe has measured, the live split is the widths' answer.
const CRUMB_KEEP: usize = 3;


/// A shallower chain NEVER folds, however cramped the cluster is — its crumbs truncate against
/// each other instead — because the smallest fold this bar allows hides two levels, and below four
/// that leaves one lonely crumb beside the ellipsis.
const FOLD_MIN_DEPTH: usize = 4;


/// The panel's row packing charges the same gap; its CSS runs a column gap of zero (the chevron IS the spacing).
const CRUMB_GAP_PX: f64 = 2.0;


const ROW_CHROME_PX: f64 = 14.0;

pub(super) fn pack_rows(widths: &[f64], budget: f64) -> Vec<usize> {
    let mut counts = vec![0usize];
    let mut used = ROW_CHROME_PX;
    for width in widths {
        let item = width + CRUMB_GAP_PX;
        if *counts.last().unwrap_or(&0) > 0 && used + item > budget {
            counts.push(0);
            used = ROW_CHROME_PX;
        }
        *counts.last_mut().unwrap() += 1;
        used += item;
    }
    counts
}

pub(super) fn row_widths(widths: &[f64], counts: &[usize]) -> Vec<f64> {
    let mut out = Vec::with_capacity(counts.len());
    let mut at = 0usize;
    for &count in counts {
        let end = (at + count).min(widths.len());
        let row = &widths[at..end];
        out.push(row.iter().sum::<f64>() + CRUMB_GAP_PX * row.len() as f64 + ROW_CHROME_PX);
        at = end;
    }
    out
}

pub(super) fn split_by_counts<T>(chain: Vec<T>, counts: &[usize]) -> Vec<Vec<T>> {
    let mut rows: Vec<Vec<T>> = Vec::with_capacity(counts.len());
    let mut rest = chain;
    for &count in counts {
        if count == 0 {
            continue;
        }
        let take = count.min(rest.len());
        rows.push(rest.drain(..take).collect());
    }
    if !rest.is_empty() {
        rows.push(rest);
    }
    rows
}


/// Never one. A single elided level costs the reader a hover to reach and costs the bar the same width as showing it would have, so the ellipsis earns its slot from two levels up.
fn elide_at(len: usize) -> usize {
    let split = len.saturating_sub(CRUMB_KEEP);
    if split == 1 {
        0
    } else {
        split
    }
}

/// Beyond the depth gate, the smallest split — never exactly one, the rule [`elide_at`] keeps — whose ellipsis and kept crumbs fit the cluster's live box, and 0 when the whole chain already does.
pub(super) fn choose_split(widths: &[f64], available: f64, len: usize) -> usize {
    if len < FOLD_MIN_DEPTH {
        return 0;
    }
    if widths.len() != len + 1 || available <= 0.0 {
        return elide_at(len);
    }
    let items = &widths[1..];
    let gap = |n: usize| CRUMB_GAP_PX * n.saturating_sub(1) as f64;
    let total: f64 = items.iter().sum::<f64>() + gap(len);
    if total <= available {
        return 0;
    }
    let ellipsis = widths[0];
    for split in 2..len {
        let suffix: f64 = items[split..].iter().sum::<f64>() + gap(len - split);
        if ellipsis + suffix + CRUMB_GAP_PX <= available {
            return split;
        }
    }
    (len - 1).max(2)
}



#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::library::breadcrumb::Crumb;

    #[test]
    fn a_shallow_chain_elides_nothing() {
        for len in 0..=CRUMB_KEEP {
            assert_eq!(elide_at(len), 0, "a chain of {len} fits the bar whole");
        }
    }

    #[test]
    fn one_elided_crumb_is_not_worth_an_affordance() {
        assert_eq!(elide_at(CRUMB_KEEP + 1), 0);
        assert_eq!(elide_at(CRUMB_KEEP + 2), 2);
    }

    #[test]
    fn past_the_first_fold_the_bar_stays_the_same_width() {
        for len in (CRUMB_KEEP + 2)..=(CRUMB_KEEP * 4) {
            let split = elide_at(len);
            assert_eq!(len - split, CRUMB_KEEP, "a chain of {len} shows only {kept}", kept = CRUMB_KEEP);
            assert!(split >= 2, "and never hides just one");
        }
    }

    #[test]
    fn the_elided_and_the_shown_are_one_chain_and_never_overlap() {
        for len in 0..12 {
            let split = elide_at(len);
            assert!(split <= len);
            assert_eq!(split + (len - split), len);
        }
    }

    fn measured(len: usize, ellipsis: f64, crumb: f64) -> Vec<f64> {
        std::iter::once(ellipsis)
            .chain(std::iter::repeat_n(crumb, len))
            .collect()
    }

    #[test]
    fn a_chain_the_cluster_holds_folds_nothing() {
        let widths = measured(4, 22.0, 120.0);
        assert_eq!(choose_split(&widths, 600.0, 4), 0);
    }

    #[test]
    fn a_shallow_chain_never_folds_however_cramped() {
        for len in 0..FOLD_MIN_DEPTH {
            let widths = measured(len, 22.0, 900.0);
            for available in [0.0, 40.0, 324.0, 5000.0] {
                assert_eq!(
                    choose_split(&widths, available, len),
                    0,
                    "a chain of {len} keeps every crumb at every width"
                );
            }
        }
    }

    #[test]
    fn a_cramped_cluster_folds_the_oldest_levels_it_must_and_no_more() {
        let widths = measured(4, 22.0, 200.0);
        assert_eq!(choose_split(&widths, 500.0, 4), 2);
        assert_eq!(choose_split(&widths, 426.0, 4), 2, "the fit is inclusive");
        assert_eq!(
            choose_split(&widths, 425.0, 4),
            3,
            "one pixel less and a level goes behind the fold"
        );
        assert_eq!(choose_split(&widths, 224.0, 4), 3);
    }

    #[test]
    fn nothing_fits_but_the_newest_level_and_the_fold_stops_there() {
        let widths = measured(4, 22.0, 200.0);
        assert_eq!(choose_split(&widths, 100.0, 4), 3, "len - 1, never past the chain");
    }

    #[test]
    fn the_fold_never_hides_exactly_one_level_whatever_the_numbers() {
        for len in 1..8 {
            let widths = measured(len, 22.0, 300.0);
            for available in [0.0, 24.0, 324.0, 646.0, 5000.0] {
                let split = choose_split(&widths, available, len);
                assert_ne!(split, 1, "one hidden level costs a hover and a bar slot");
                assert!(
                    split <= len,
                    "split_at({split}) on a chain of {len} would panic"
                );
            }
        }
    }

    #[test]
    fn unmeasured_numbers_fall_back_to_the_count_rule() {
        assert_eq!(choose_split(&[], 500.0, 6), elide_at(6));
        assert_eq!(
            choose_split(&measured(4, 22.0, 100.0), 500.0, 6),
            elide_at(6),
            "a probe out of step with the chain is not trusted"
        );
        assert_eq!(
            choose_split(&measured(5, 22.0, 100.0), 0.0, 5),
            elide_at(5),
            "a cluster with no measured box folds by count"
        );
    }

    #[test]
    fn one_crumb_that_does_not_fit_is_truncated_not_hidden() {
        let widths = measured(1, 22.0, 900.0);
        assert_eq!(
            choose_split(&widths, 100.0, 1),
            0,
            "hiding the only level behind an ellipsis leaves the bar with nowhere to go"
        );
    }

    #[test]
    fn rows_pack_to_the_budget_and_later_rows_start_fresh() {
        assert_eq!(pack_rows(&[100.0, 100.0, 100.0], 220.0), vec![2, 1]);
        assert_eq!(
            pack_rows(&[100.0, 100.0, 100.0], 1000.0),
            vec![3],
            "under the budget it is one row"
        );
        assert_eq!(
            pack_rows(&[500.0, 100.0], 200.0),
            vec![1, 1],
            "a crumb wider than the budget gets a row to itself; it is never dropped"
        );
        assert_eq!(
            pack_rows(&[], 220.0).iter().sum::<usize>(),
            0,
            "no crumbs, no rows worth of them"
        );
    }

    #[test]
    fn a_packed_row_is_as_wide_as_its_own_labels_plus_chrome() {
        let widths = [100.0, 100.0, 100.0];
        let counts = pack_rows(&widths, 220.0);
        let rows = row_widths(&widths, &counts);
        assert_eq!(rows, vec![218.0, 116.0]);
        assert_eq!(
            rows.into_iter().fold(0.0, f64::max),
            218.0,
            "the panel is the width of its widest row and no wider"
        );
    }

    #[test]
    fn splitting_a_chain_by_counts_never_loses_a_level() {
        let chain: Vec<Crumb> = (0..5)
            .map(|at| Crumb {
                id: format!("s{at}"),
                name: format!("Level {at}"),
                watched: false,
            })
            .collect();
        let rows = split_by_counts(chain, &[2, 2]);
        assert_eq!(
            rows.len(),
            3,
            "the leftover rides a tail row rather than vanishing"
        );
        assert_eq!(rows.iter().flatten().count(), 5);
        assert_eq!(rows[0][0].id, "s0");
        assert_eq!(rows[1][0].id, "s2");
        assert_eq!(rows[2][0].id, "s4");
    }
}
