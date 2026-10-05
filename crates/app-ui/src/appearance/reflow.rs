//! The reflowable pipeline's appearance hooks. Its pages need no engine
//! call — the tokens repaint reactively from the CSS variables — so this
//! side of the pipeline is one pure step: recomputing the page palette
//! (with the ink dial resolved in Rust, not in the stylesheet).

use reader_core::appearance::Appearance;
use reader_core::appearance::reflowable::tokens;

/// The `--tx-*` variables for an appearance and the ink-contrast dial
/// (0..=100). Always painted alongside the raster token set — the two
/// namespaces are disjoint, so whichever format is open finds its own
/// tokens waiting.
pub fn token_vars(a: &Appearance, ink_contrast: f64) -> Vec<(&'static str, String)> {
    tokens::css_variables(a, ink_contrast)
}
