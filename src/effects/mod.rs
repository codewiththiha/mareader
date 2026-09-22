//! Reactive effects, grouped by domain: app-level concerns in `app` (the
//! window and browser arms, the theme and typography appliers, the library's
//! automatic moments and the reading-position write-back), and the appearance
//! scrub/commit scheduler here, which serves both surfaces.
//!
//! The reader's own effects are `reader_app::effects`: they keep an open
//! document in sync, and there is nothing for them to do in a window with
//! none. Two of them are installed from here anyway — `effects::app::reading_progress`
//! lives in this crate because it writes the library, and the rest are
//! installed once at boot by `reader_app::effects::install`.

pub mod app;
pub mod appearance;
