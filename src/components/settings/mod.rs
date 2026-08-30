//! The reader settings modal and its tabs: `modal` is the shell (the tab
//! strip, the close affordance, the Escape handler), `common` is what the
//! tabs share, and `layout` / `theme` / `animations` are the tabs themselves.
//!
//! This is its own component group rather than another `menus` sibling because
//! a settings tab is not a menu: the tabs are peers hosted by a modal, and the
//! set of them is not fixed — the Animations tab only exists while the master
//! switch in the Layout tab is on. One module per tab is what lets a tab
//! appear and disappear without any other tab knowing.
//!
//! Two rules every tab keeps:
//!
//! * A control binds to exactly one field of `Settings` and writes it through
//!   `state.settings`. There is no local copy to drift: `pdf_core::settings`'
//!   field names ARE the persisted schema, so a row that works also saves.
//! * A control another switch disables takes `disabled=` a derived signal, so
//!   the row stays visible and explains itself instead of vanishing.

pub(crate) mod animations;
pub(crate) mod common;
pub(crate) mod layout;
pub mod modal;
pub(crate) mod theme;
