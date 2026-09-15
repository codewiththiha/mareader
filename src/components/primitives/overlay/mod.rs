//! Overlay primitives: toasts (data model + visual + host controller), the
//! floating action bar, the modal sheet's chrome, the heading, body and
//! button row inside it, the question sheet that composes them for every
//! "the library has a question" surface, and the lane registry that decides
//! which surfaces may be up at the same time.

pub mod action_bar;
pub mod lanes;
pub mod modal_shell;
pub mod question_sheet;
pub mod sheet;
pub mod toast;
pub mod toast_host;
