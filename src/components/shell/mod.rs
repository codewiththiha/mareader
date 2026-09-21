//! The unified application shell: the chrome that wraps a page rather than
//! the page's content.
//!
//!   * `sidebar` — the rail family: the shared aside container, the two
//!     mount points (docked and floating), the header, the identity row,
//!     the switcher and the panel hosts.
//!   * `titlebar` — the bar family: the generic hover/pin shell, the app
//!     wiring around it, the native traffic lights, the document titles and
//!     the popover policy toolbar menus share.
//!
//! One part of this family is not here: the layout rulebook every chrome
//! component asks — [`ShellController`] — is `app_chrome::controller`, because
//! whichever half of the app is rendering needs it and neither may own it.
//!
//! The shell is deliberately separate from the reader (`features/reader`):
//! pages, zoom, search and virtualization are the reader's business; the
//! shell only owns the frame around them.

pub mod sidebar;
pub mod titlebar;

use app_chrome::controller::{ChromeSurface, ShellController};
use leptos::prelude::*;

use crate::state::AppState;

/// Build the surface's layout controller out of the app's state.
///
/// The controller itself takes four values and knows nothing about an app
/// (that is what lets the reader build its own later, over a document rather
/// than a window); this is the one place the app's state is unpacked into
/// them. The motion projection is the reader's because that is where the app
/// publishes it — one signal, written once at the root, read by every surface
/// that moves.
pub(crate) fn controller_for(state: AppState, surface: ChromeSurface) -> ShellController {
    ShellController::build(
        state.settings,
        state.ui.sidebar,
        Signal::derive(move || state.reader.viewer.motion.get()),
        surface,
    )
}
