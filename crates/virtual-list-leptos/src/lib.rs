//! Reactive virtual scrolling for Leptos, on the `virtual-list` kernel.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod engine;
mod hook;
mod observe;
mod options;
mod render;
pub mod retention;
mod surface;
mod virtualizer;

pub use crate::engine::{CoreConfig, Flush, Step, VirtualizerCore};
pub use crate::hook::use_virtualizer;
pub use crate::options::{Axis, LayoutShape, ScrollMode, VirtualizerOptions};
pub use crate::render::{VirtualItem, VirtualItemState, VirtualRow};
pub use crate::retention::RetentionPolicy;
pub use crate::surface::{DomSurface, ScrollSurface};
pub use crate::virtualizer::Virtualizer;
pub use virtual_list::{Align, BandWindow, Direction, FillPriority, Pipeline};
