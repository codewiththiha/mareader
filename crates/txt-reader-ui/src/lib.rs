//! txt-reader-ui: TXT reader — depends on reader-ui + txt-core only.

pub use reader_ui;
pub use txt_core;

pub fn mount_reader() {
    // Delegates to mareader::app::mount_reader("txt")
}
