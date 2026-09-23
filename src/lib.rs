//! The app as a library, so a format artifact can mount the view without Trunk
//! building a second target. The host bin (`src/main.rs`) does not enable a
//! format feature, so it does not compile the view. Trunk is pinned to that
//! bin (`data-bin="mareader"`). This crate is an rlib, never a cdylib: a
//! cdylib here would collide with the bin's wasm name.

mod app;
mod boot;
mod components;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
mod dom_contract;
mod effects;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
mod epoch;
mod events;
mod features;
mod memory;
mod services;
mod slot;
mod state;
mod storage;
mod time;
mod zoom;

#[cfg(format_runtime)]
pub mod format_runtime;
