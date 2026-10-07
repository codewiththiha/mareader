//! Page-paper colour logic for the blend backdrop.
mod color;
mod config;
mod detect;
mod palette;

pub use color::{Rgb, lerp};
pub use config::{DEFAULT_EDGE_WIDTH, PaperArea, PaperConfig};
pub use detect::{PAPER_SHARE, PaperDetector};
pub use palette::PagePalette;
