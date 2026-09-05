//! The appearance kernel's shared helpers: the colour-space maths, the
//! tint hue mapping and the noise/texture emitters. Both pipelines
//! ([`super::raster`] and [`super::reflowable`]) consume these; neither owns
//! them.

pub mod noise;
pub mod oklch;
pub mod texture;
pub mod tint;
