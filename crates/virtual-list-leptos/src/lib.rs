//! Reactive virtual scrolling for Leptos, built on the `virtual-list`
//! geometry kernel.
//!
//! The crate splits in two halves:
//!
//! - [`engine::VirtualizerCore`] — a **pure state machine**: windowing,
//!   measurement flushes, scroll anchoring, and scroll-to with retry. No
//!   signals, no DOM, fully unit-tested on the host.
//! - [`use_virtualizer`] — the Leptos wiring: rAF-coalesced scroll handling,
//!   epsilon-guarded `ResizeObserver` writes, write-if-changed signals, and
//!   cleanup on disposal.
//!
//! # Example (continuous list)
//!
//! ```ignore
//! let v = use_virtualizer(
//!     VirtualizerOptions::list(num_pages, move |i| height_of(i))
//!         .gap(PAGE_GAP)
//!         .budget(Budget::screenfuls(0.5, 3))
//!         .padding(48.0, 0.0)
//!         .pinned(selection_pin),
//! );
//!
//! view! {
//!     <div node_ref=container class="overflow-y-auto">
//!         <div style:position="relative">
//!             <div aria-hidden="true"
//!                  style:height=move || format!("{}px", v.total_size().get()) />
//!             <For
//!                 each=move || v.items().get()
//!                 key=|item| item.index
//!                 children=move |item| view! {
//!                     <div style=format!("position:absolute;top:{}px", item.start)>
//!                         <MyCell index=item.index measure=v.clone() />
//!                     </div>
//!                 }
//!             />
//!         </div>
//!     </div>
//! }
//! // once the container node exists:
//! v.bind_container(container_el);
//! ```
//!
//! Grids use [`VirtualizerOptions::grid`] and render
//! [`rows`](Virtualizer::rows) instead of items; width-driven column counts
//! come from [`GridSpec::responsive`].

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
pub use crate::surface::{DomSurface, ScrollSurface};
pub use crate::virtualizer::Virtualizer;
pub use virtual_list::Align;
