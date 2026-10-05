//! The rail's two panels, one directory each: `outline` and `thumbnails`.
//!
//! Both have the same shape — `panel` is the reusable surface, `view` is the
//! wrapper the rail mounts (the absolutely-stacked host with its paint/outro
//! toggles) — and `thumbnails` additionally holds the grid's internals
//! (`geometry`, `thumbnail_cell`, `auto_center`).

pub mod outline;
pub mod thumbnails;
