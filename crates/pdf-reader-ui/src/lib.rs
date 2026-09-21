//! pdf-reader-ui: PDF-specific reader. The ONLY crate that may depend on pdf-engine.
//! Dependency graph: reader-ui + pdf-core + pdf-engine + pdf-paper

pub use pdf_core;
pub use pdf_engine;
pub use pdf_paper;
pub use reader_ui;

/// Mount the PDF reader.
pub fn mount_reader() {
    // Delegates to mareader::app::mount_reader("pdf") when built with pdf feature.
}

pub mod thumbnail_source {
    //! PDF thumbnail provider implements ui_common::ThumbnailSource via pdf-engine.
    use super::*;
}
