//! Shared shells that surround a layout: [`PageShell`] for the paginated
//! modes and [`ScrollShell`] for the two scrolling modes. Each shell owns the
//! chrome that is common to its family (scroller, scrollbar, the horizontal
//! wheel help, the reading progress strip) so the layouts stay thin.
//!
//! [`anchor_settle`] is the family's other shared piece, and the reflowable
//! stream borrows it too: the loop that re-asserts a fresh mount's scroll
//! position until the browser has committed the container's box.

pub mod anchor_settle;
pub mod page_shell;
pub mod scroll_shell;
