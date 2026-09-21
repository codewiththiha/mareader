//! Reactive effects, grouped by domain: app-level concerns in `app` and
//! the appearance scrub/commit scheduler here (it serves both surfaces).
//! The reader systems (navigation sync, zoom watchers, the reflow
//! pipeline, search) moved to the reader crate (`reader_app::effects`).

pub mod app;
pub mod appearance;
