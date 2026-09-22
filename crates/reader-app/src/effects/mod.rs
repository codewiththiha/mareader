//! Reader effects: the reactive systems that keep the reader in sync
//! (scroll/page navigation sync, zoom sources, search, selection tracking,
//! the layout preferences, the mode flip, the reflow measurement pipeline).
//!
//! Two groups, installed from two places:
//!
//!   * the four app-lifetime arms, installed once at boot by [`install`] —
//!     they must all be running before the FIRST document opens, which is
//!     earlier than any reader mounts;
//!   * the rest, installed per reader by
//!     [`crate::features::root::ReaderRoot`] in an order that is a contract
//!     rather than a habit (the layout prefs resolve the gap the reflow
//!     layout reads; the zoom controller is driven before the watchers that
//!     post to it; `navigation_sync` replays a held jump before anything
//!     persists the page).
//!
//! The reading-position write-back is NOT here: it writes the library's rows
//! and their persisted blob, so it is the shell's effect and the shell
//! installs it — at the point in this order that the zoom contract asks for,
//! through the callback it hands the reader root.

use leptos::prelude::*;

use reader_core::settings::Settings;

use crate::state::ReaderState;

pub mod auto_scroll;
pub mod blend_backdrop;
pub mod first_paint;
pub mod layout_prefs;
pub mod link_navigation;
pub mod mode_change;
pub mod navigation_sync;
pub mod page_selection;
pub mod reflow_layout;
pub mod reflow_measure;
pub mod reflow_outline;
pub mod search;
pub mod selection_tracking;
pub mod zoom_watchers;

/// The reader's app-lifetime effects, installed once by the shell at boot.
///
/// `paper_settings` is the reason this exists as a call the shell makes
/// BEFORE it opens anything: the blend and detection settings must reach the
/// engine's paper session ahead of the first document, because the open flow
/// asks the per-document colour cache under the reader's real settings — and
/// asked under defaults, the first book's backdrop is quietly the wrong
/// colour. The other three are input and selection arms that only need to be
/// up before the reader is interactable.
pub fn install(reader: ReaderState, settings: RwSignal<Settings>) {
    blend_backdrop::paper_settings(settings);
    link_navigation::link_navigation(reader);
    page_selection::page_selection(reader);
    selection_tracking::selection_tracking(reader);
}
