//! Scaling check: compare the three virtual-list backends for offset lookups,
//! window queries, and dynamic size updates.
//!
//! Run with: `cargo bench -p virtual-list --features advanced-trees` (or `cargo run --bench scaling_check
//! -p virtual-list --release --features advanced-trees`).
use std::hint::black_box;
use std::time::Instant;
use virtual_list::{Budget, ChunkedStrip, FenwickStrip, Strip};

fn linear_offset(i: usize, h: &[f64], gap: f64) -> f64 {
    h.iter().take(i).sum::<f64>() + gap * i as f64
}

fn main() {
    for n in [100usize, 1_000, 5_000, 20_000] {
        let heights: Vec<f64> = (0..n).map(|i| 700.0 + (i % 7) as f64 * 30.0).collect();
        let strip = Strip::new(heights.clone(), 24.0);
        let _fenwick = FenwickStrip::new(heights.clone(), 24.0);
        let _chunked = ChunkedStrip::new(heights.clone(), 24.0);

        // 1) Baseline: linear scan vs prefix-sum offset for every item.
        let t = Instant::now();
        let mut acc = 0.0;
        for i in 0..n {
            acc += linear_offset(i, &heights, 24.0);
        }
        let lin = t.elapsed();

        let t = Instant::now();
        let mut acc2 = 0.0;
        for i in 0..n {
            acc2 += strip.offset(i);
        }
        let pre = t.elapsed();
        assert!((acc - acc2).abs() < 1.0);

        // 2) Hinted vs unhinted `index_at` over a continuous sweep.
        let mut hint = 0usize;
        let t = Instant::now();
        let mut acc3 = 0usize;
        for i in 0..n {
            let pos = (i as f64) * 17.3 % strip.total().max(1.0);
            acc3 += strip.index_at_hinted(pos, &mut hint);
        }
        let hinted = t.elapsed();

        let t = Instant::now();
        let mut acc4 = 0usize;
        for i in 0..n {
            let pos = (i as f64) * 17.3 % strip.total().max(1.0);
            acc4 += strip.index_at(pos);
        }
        let unhinted = t.elapsed();
        // hint correctness
        assert_eq!(acc3, acc4, "hinted and unhinted disagree");

        // 3) Dynamic updates: `set_size` on FenwickStrip vs ChunkedStrip vs Strip.
        let mut fen = FenwickStrip::new(heights.clone(), 24.0);
        let mut chu = ChunkedStrip::new(heights.clone(), 24.0);
        let mut strp = Strip::new(heights.clone(), 24.0);
        let mut indices: Vec<usize> = (0..n).step_by(7).collect();
        // LCG to scramble indices so we hit different chunks.
        for (i, idx) in indices.iter_mut().enumerate() {
            *idx = (idx.wrapping_mul(2654435761).wrapping_add(i * 7)) % n;
        }
        let t = Instant::now();
        for &i in &indices {
            fen.set_size(i, 800.0 + (i % 13) as f64 * 17.0);
        }
        let fen_t = t.elapsed();

        let t = Instant::now();
        for &i in &indices {
            chu.set_size(i, 800.0 + (i % 13) as f64 * 17.0);
        }
        let chu_t = t.elapsed();

        let t = Instant::now();
        for &i in &indices {
            strp.set_size(i, 800.0 + (i % 13) as f64 * 17.0);
        }
        let strp_t = t.elapsed();

        // 4) Window query on each backend.
        let win_stripped = strip.window(strip.total() / 2.0, 900.0, Budget::default());
        let t = Instant::now();
        for _ in 0..1_000 {
            let _ =
                black_box(strip.window(black_box(strip.total() / 2.0), 900.0, Budget::default()));
        }
        let win_strip_t = t.elapsed();

        let t = Instant::now();
        for _ in 0..1_000 {
            let _ = black_box(fen.window(black_box(fen.total() / 2.0), 900.0, Budget::default()));
        }
        let win_fen_t = t.elapsed();

        let t = Instant::now();
        for _ in 0..1_000 {
            let _ = black_box(chu.window(black_box(chu.total() / 2.0), 900.0, Budget::default()));
        }
        let win_chu_t = t.elapsed();

        println!();
        println!(
            "n={n:5}  position-all (linear {lin:>10.2?} vs prefix {pre:>10.2?})  speedup {:>6.1}x",
            lin.as_secs_f64() / pre.as_secs_f64().max(1e-12)
        );
        println!(
            "        index_at sweep: hinted {hinted:>10.2?}  unhinted {unhinted:>10.2?}  speedup {:>5.1}x",
            unhinted.as_secs_f64() / hinted.as_secs_f64().max(1e-12)
        );
        println!(
            "        set_size x{n_updates}:  Fenwick {fen_t:>10.2?}  Chunked {chu_t:>10.2?}  Strip {strp_t:>10.2?}",
            n_updates = indices.len()
        );
        println!(
            "        window x1000:           Strip {win_strip_t:>10.2?}  Fenwick {win_fen_t:>10.2?}  Chunked {win_chu_t:>10.2?}  ({win_stripped:?})",
            win_stripped = win_stripped.map(|w| (w.first, w.last))
        );
    }
}
