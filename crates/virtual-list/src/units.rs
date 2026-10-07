//! Sub-pixel units: the integer representation behind [`crate::Strip`]'s
//! prefix sums.

/// Sub-pixel units per logical pixel: 2^16 keeps common UI
/// coordinates exact in i64.
const SUBPIXEL_BITS: u32 = 16;

/// `2 ** SUBPIXEL_BITS`, the multiply and divide factor.
pub(crate) const SUBPIXEL_FACTOR: i64 = 1 << SUBPIXEL_BITS;

/// Convert an `f64` measurement into `i64` sub-pixels.
#[inline]
pub(crate) fn to_sub(px: f64) -> i64 {
    if px.is_nan() || px <= 0.0 {
        return 0;
    }
    if px.is_infinite() {
        return i64::MAX;
    }
    // px is finite and strictly positive.
    let v = px * (SUBPIXEL_FACTOR as f64);
    if v >= (i64::MAX as f64) {
        i64::MAX
    } else {
        v as i64
    }
}

/// Convert `i64` sub-pixels back into `f64` CSS pixels.
#[inline]
pub(crate) fn from_sub(sub: i64) -> f64 {
    (sub as f64) / (SUBPIXEL_FACTOR as f64)
}
