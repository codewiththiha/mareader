//! Windowing math for virtualized lists of variably-sized items.

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

pub mod anchor;
pub mod backend;
mod layout;
pub mod motion;
mod units;
mod window;

pub use anchor::{AnchorPolicy, correct, pin_at, rescale_anchor};
pub use backend::{Strip, StripBackend};
pub use layout::{GridColumns, GridLayout, GridSpec, Layout, LayoutKind, ListLayout};
pub use motion::{BandRange, BandWindow, Direction, FillPriority, Motion, MotionConfig, Pipeline};
pub use window::{Align, Budget, Overscan, Viewport, Window};
