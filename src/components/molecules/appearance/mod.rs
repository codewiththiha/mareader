//! Sections of the 🎨 Appearance popover, one file per control group.
//!
//! Split out of the old flat molecules/ layout because the appearance surface
//! grew from "three short lists" to presets + tint + two texture sliders +
//! grain modes; keeping them together would have made one long file whose
//! sections had nothing to do with each other.

pub mod base_section;
pub mod noise_section;
pub mod preset_section;
pub mod texture_section;
