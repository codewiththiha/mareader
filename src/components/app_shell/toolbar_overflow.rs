//! Pure collapse math for the adaptive toolbar. No DOM.

/// Indices that must move to the overflow menu. Deterministic: lowest
/// priority first, ties drop the right-most item.
pub fn compute_collapsed(
    widths: &[f64],
    priorities: &[u32],
    capacity: f64,
    gap: f64,
    overflow_w: f64,
) -> Vec<usize> {
    let n = widths.len();
    if n == 0 {
        return vec![];
    }
    let total: f64 = widths.iter().sum::<f64>() + gap * (n.saturating_sub(1) as f64);
    if total <= capacity {
        return vec![];
    }
    let budget = capacity - overflow_w - gap;
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| priorities[a].cmp(&priorities[b]).then(b.cmp(&a)));
    let mut dropped = vec![false; n];
    let mut used = total;
    for &i in &order {
        if priorities[i] == u32::MAX {
            continue;
        }
        if used <= budget {
            break;
        }
        used -= widths[i] + gap;
        dropped[i] = true;
    }
    (0..n).filter(|&i| dropped[i]).collect()
}

#[cfg(test)]
mod tests {
    use super::compute_collapsed;

    #[test]
    fn all_fit_returns_empty() {
        let widths = [40.0, 40.0, 40.0];
        let prios = [80, 80, 90];
        assert!(compute_collapsed(&widths, &prios, 200.0, 4.0, 36.0).is_empty());
    }

    #[test]
    fn tight_drops_lowest_priority_first() {
        let widths = [40.0, 40.0, 40.0];
        let prios = [90, 80, 70];
        let dropped = compute_collapsed(&widths, &prios, 125.0, 4.0, 36.0);
        assert!(dropped.contains(&2));
        assert_eq!(dropped.first().copied(), Some(2));
    }

    #[test]
    fn essential_never_dropped() {
        let widths = [40.0, 80.0, 40.0];
        let prios = [70, u32::MAX, 80];
        let dropped = compute_collapsed(&widths, &prios, 50.0, 4.0, 36.0);
        assert!(!dropped.contains(&1));
    }

    #[test]
    fn identical_inputs_are_stable() {
        let widths = [36.0, 36.0, 64.0, 48.0, 40.0];
        let prios = [80, 80, u32::MAX, 90, 70];
        let a = compute_collapsed(&widths, &prios, 140.0, 4.0, 36.0);
        let b = compute_collapsed(&widths, &prios, 140.0, 4.0, 36.0);
        assert_eq!(a, b);
    }

    #[test]
    fn ties_drop_rightmost_first() {
        let widths = [50.0, 50.0];
        let prios = [70, 70];
        let dropped = compute_collapsed(&widths, &prios, 90.0, 4.0, 36.0);
        assert_eq!(dropped, vec![1]);
    }
}
