//! The reflowable pipeline's appearance hooks: the page palette.

use reader_core::appearance::Appearance;
use reader_core::appearance::reflowable::tokens;

/// The `--tx-*` variables for an appearance and the ink dial.
pub fn token_vars(a: &Appearance, ink_contrast: f64) -> Vec<(&'static str, String)> {
    tokens::css_variables(a, ink_contrast)
}
