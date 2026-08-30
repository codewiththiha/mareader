//! The titlebar family: the generic hover/grab bar shell (`root`), the app
//! wiring that adapts it to this application (`app_title_bar`), the native
//! traffic lights, the document titles, and the popover policy the toolbar
//! menus share.

pub mod app_title_bar;
pub mod constants;
pub mod document_title;
pub mod floating_document_title;
pub mod root;
pub mod toolbar_popover;
pub mod traffic_lights;
