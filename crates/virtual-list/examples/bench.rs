// Rough scaling check: old O(n) linear scan vs Strip's prefix-sum lookups.
use std::time::Instant;
use virtual_list::{Budget, Strip};

fn linear_offset(i: usize, h: &[f64], gap: f64) -> f64 {
    h.iter().take(i).sum::<f64>() + gap * i as f64
}

fn main() {
    for n in [100usize, 1_000, 5_000] {
        let heights: Vec<f64> = (0..n).map(|i| 700.0 + (i % 7) as f64 * 30.0).collect();
        let strip = Strip::new(heights.clone(), 24.0);

        // Simulate one frame: position every item.
        let t = Instant::now();
        let mut acc = 0.0;
        for i in 0..n { acc += linear_offset(i, &heights, 24.0); }
        let lin = t.elapsed();

        let t = Instant::now();
        let mut acc2 = 0.0;
        for i in 0..n { acc2 += strip.offset(i); }
        let pre = t.elapsed();

        assert!((acc - acc2).abs() < 1.0);
        let w = strip.window(strip.total()/2.0, 900.0, Budget::default()).unwrap();
        println!("n={n:5}  position-all: linear {lin:>10.2?}  prefix {pre:>10.2?}  speedup {:>6.1}x   window={:?}",
                 lin.as_secs_f64()/pre.as_secs_f64().max(1e-12), (w.first, w.last));
    }
}
