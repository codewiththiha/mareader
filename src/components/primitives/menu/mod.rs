//! The menu chrome: item, separator, section label, shortcut hint and the
//! key cap, plus the choice row a question sheet's answers use. They live as
//! one group because menus, popovers, settings sections and the library's
//! question sheets reach for them together.

pub mod kbd;
pub mod menu_item;
pub mod choice_row;
pub mod section_label;
pub mod separator;
pub mod shortcut_row;
