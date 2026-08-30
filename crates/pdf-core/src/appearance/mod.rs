//! Appearance: base mode, colour tint, texture, noise — and the maths that
//! turns them into CSS values.
//!
//!   * `model`   — the data model (modes + [`Appearance`]), the persisted schema
//!   * `tint`    — colour maths: filter pipeline, blend, tinted UI tokens
//!   * `preview` — the preset-thumbnail preview style/class

mod model;
mod preview;
mod tint;

pub use model::{Appearance, BaseMode, NoiseMode, TextureMode};
