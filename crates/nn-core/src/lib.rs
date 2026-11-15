//! Small inference primitives shared by the neural voice crates.

pub mod noise;
pub mod simd;
pub mod wire;

/// Implement the common voice-model error messages for a crate's `Error`.
#[macro_export]
macro_rules! voice_error_display {
    () => {
        impl std::fmt::Display for Error {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    Self::Load(why) => write!(f, "couldn't load the voice model: {why}"),
                    Self::Input(why) => write!(f, "couldn't speak this: {why}"),
                    Self::Compute(why) => write!(f, "the voice model failed: {why}"),
                }
            }
        }
    };
}
