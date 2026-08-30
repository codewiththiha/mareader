//! Shared shells that surround a layout: [`PageShell`] for the paginated
//! modes and [`ScrollShell`] for the two scrolling modes. Each shell owns the
//! chrome that is common to its family (scroller, scrollbar, the horizontal
//! wheel help, the reading progress strip) so the layouts stay thin.

pub mod page_shell;
pub mod scroll_shell;
