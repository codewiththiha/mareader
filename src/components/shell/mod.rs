//! The unified application shell: the chrome that wraps a page rather than
//! the page's content. Grouped into three parts:
//!
//!   * `controller` — [`ShellController`](controller::ShellController), the
//!     single source of truth for shell layout state. Pages build one and
//!     provide it as context; every "is the rail overlay?", "does the bar
//!     owe the lights a gutter?" question is asked of it, never recomputed.
//!   * `titlebar` — the bar family: the generic hover/pin shell, the app
//!     wiring around it, the native traffic lights, the document titles and
//!     the popover policy toolbar menus share.
//!   * `sidebar` — the rail family: the shared aside container, the two
//!     mount points (docked and floating), the header, the identity row,
//!     the switcher and the panel hosts.
//!
//! The shell is deliberately separate from the reader (`features/reader`):
//! pages, zoom, search and virtualization are the reader's business; the
//! shell only owns the frame around them.

pub mod controller;
pub mod sidebar;
pub mod titlebar;
