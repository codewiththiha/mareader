//! The anchored floating surfaces: popover, context menu, the floating
//! card, and the app's menu popover — the popover plus the two policies
//! (titlebar hold, overlay lane) every anchored menu shares.
//!
//! The plumbing they sit on is chrome and lives in the `app-chrome` crate:
//! placement glue ([`app_chrome::floating::position`]), shared dismissal
//! mechanics ([`app_chrome::floating::dismiss`]) and the element/geometry
//! helpers ([`app_chrome::floating::types`]), with the pure placement math
//! in `ui_geom::floating`. These three modules are the views — the
//! components that decide what a popover, a menu or a morphing card looks
//! like, composed out of that plumbing.

pub mod context_menu;
pub mod floating_card;
pub mod menu_popover;
pub mod popover;
