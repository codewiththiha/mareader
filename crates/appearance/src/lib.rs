//! The reading look as data and colour maths; no egui, no wgpu (L0).
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::unimplemented,
    clippy::todo,
    clippy::unreachable,
    clippy::indexing_slicing
)]

mod color;
mod filter;
mod glow;
mod grain;
mod mark;
mod model;
mod pane_looks;
mod pattern;
mod presets;
mod toast;
mod tokens;

pub use color::{Oklch, Rgb};
pub use filter::{Blend, ColorMatrix, FilterOp, MAX_FILTER_OPS, PageTone, apply_filter};
pub use glow::GlowPalette;
pub use grain::{CRAWL, CRAWL_SECS, FilmGrain, NOISE_FREQUENCY, NOISE_TILE, crawl, layer};
pub use mark::{MARK_CONTRAST, MarkFill};
pub use model::{Appearance, BaseMode, FULL_TINT, NoiseMode, TextureMode};
pub use pane_looks::{PANE_TINT_STRENGTH, PaneLooks, Routing};
pub use pattern::{
    DOT_PITCH, GRAIN_ALPHA, GRAIN_FREQUENCY, GRAIN_TILE, LINE_PITCH, PATTERN_PERIOD, Pattern,
};
pub use presets::{
    Preset, PresetGroup, UNSECTIONED, builtin_presets, group_presets, preset_id, section_names,
};
pub use toast::{ToastPalette, ToastSwatch};
pub use tokens::{Marks, TextPalette, UiTokens, base_tokens, legible};
