//! Overlay primitives: toasts (data model + visual + host controller), the
//! floating action bar, and the lane registry that decides which surfaces may
//! be up at the same time.

pub mod action_bar;
pub mod lanes;
pub mod toast;
pub mod toast_host;
