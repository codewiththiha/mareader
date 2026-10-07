//! The reader's format-agnostic domain: formats, settings, appearance,
//! view maths.

pub mod appearance;
pub mod document;
pub mod filename;
pub mod format;
pub mod outline;
pub mod search;
pub mod settings;
pub mod view;
pub mod zoom_math;

pub use format::{
    DocumentKind, Format, SUPPORTED, extensions, first_supported, format_from_ext, format_of,
    is_supported_mime, is_supported_path, kind_list, kind_names,
};
pub use outline::OutlineNode;
