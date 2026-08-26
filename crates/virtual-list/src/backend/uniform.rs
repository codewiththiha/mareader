//! Zero-storage uniform backend: `O(1)` memory instead of `O(n)` for
//! grids or other uniform runs.

use crate::units::{from_sub, to_sub};

/// A uniform strip: `count` items, each `size` pixels, with fixed `gap`.
/// No per-item storage — all queries compute from the parameters directly.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct UniformStrip {
    n: usize,
    size_sub: i64,
    gap_sub: i64,
}

impl UniformStrip {
    /// Build a uniform strip.
    pub fn new(count: usize, size: f64, gap: f64) -> Self {
        Self {
            n: count,
            size_sub: to_sub(size),
            gap_sub: to_sub(gap),
        }
    }

    /// Number of items.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether empty.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Offset of item `index`.
    pub fn offset(&self, index: usize) -> f64 {
        if index == 0 || index >= self.n {
            return if index == 0 { 0.0 } else { self.total() };
        }
        from_sub((self.size_sub + self.gap_sub) * index as i64)
    }

    /// Size of item `index`.
    pub fn size(&self, index: usize) -> f64 {
        if index >= self.n {
            return 0.0;
        }
        from_sub(self.size_sub)
    }

    /// Total extent.
    pub fn total(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        from_sub(self.size_sub * self.n as i64 + self.gap_sub * (self.n as i64 - 1))
    }

    /// Index at sub-pixel position.
    pub fn index_at(&self, pos: f64) -> usize {
        if self.n == 0 || pos <= 0.0 {
            return 0;
        }
        let p = to_sub(pos);
        let pitch = self.size_sub + self.gap_sub;
        if pitch == 0 {
            return 0;
        }
        let i = ((p / pitch) as usize).min(self.n - 1);
        let start_sub = pitch * i as i64;
        let end_sub = start_sub + self.size_sub;
        if p >= end_sub && i + 1 < self.n {
            i + 1
        } else {
            i
        }
    }
}

impl super::StripBackend for UniformStrip {
    fn len(&self) -> usize {
        self.n
    }

    fn gap_sub(&self) -> i64 {
        self.gap_sub
    }

    fn offset_sub(&self, index: usize) -> i64 {
        if index == 0 {
            return 0;
        }
        if index >= self.n {
            return self.total_sub();
        }
        (self.size_sub + self.gap_sub) * index as i64
    }

    fn size_sub(&self, index: usize) -> i64 {
        if index >= self.n {
            return 0;
        }
        self.size_sub
    }

    fn total_sub(&self) -> i64 {
        if self.n == 0 {
            return 0;
        }
        self.size_sub * self.n as i64 + self.gap_sub * (self.n as i64 - 1)
    }

    fn index_at_sub(&self, p: i64) -> usize {
        if self.n == 0 || p <= 0 {
            return 0;
        }
        let pitch = self.size_sub + self.gap_sub;
        if pitch == 0 {
            return 0;
        }
        let i = ((p / pitch) as usize).min(self.n - 1);
        let start_sub = pitch * i as i64;
        let end_sub = start_sub + self.size_sub;
        if p >= end_sub && i + 1 < self.n {
            i + 1
        } else {
            i
        }
    }

    fn set_size_sub(&mut self, _index: usize, _new_sub: i64) -> i64 {
        0 // uniform = no per-item resize
    }
}
