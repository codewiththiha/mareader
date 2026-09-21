//! The reader's reusable components, split from the shell: the viewer slot
//! (`viewer`), the format page hosts it switches on (`formats`), the
//! floating search (`search`), the AI selection surfaces (`ai`), and the
//! rail family (`rail`). The shell keeps its own title bar, menus and
//! settings modal; a component that turns out window-shaped belongs there,
//! not here.

pub mod ai;
pub mod formats;
pub mod rail;
pub mod search;
pub mod viewer;
