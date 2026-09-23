//! Reactive effects, grouped by domain: app-level concerns in `app`,
//! reader systems in `reader`, and the appearance scrub/commit
//! scheduler here (it serves both surfaces).

pub mod app;
pub mod appearance;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub mod reader;
