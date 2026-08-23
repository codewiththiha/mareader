//! Reactive effects, grouped by domain: app-level concerns in `app`,
//! reader systems in `reader`, and the appearance scrub/commit
//! scheduler here (it serves both surfaces).

pub mod app;
pub mod appearance;
pub mod reader;
