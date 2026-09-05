//! Sub-pixel units: the integer representation behind [`crate::Strip`]'s
//! prefix sums.

/// Number of sub-pixel units per logical pixel. `2^16 = 65536` gives an exact
/// representation for every value whose denominator is a power of two up to
/// `65536` — i.e. every common UI coordinate (`0.25`, `0.5`, `0.75`, sub-pixel
/// `1/64` font hinting, etc.) — and keeps `i64` arithmetic exact for any
/// total extent below ~4.2e9 CSS pixels (~4 200 km).
const SUBPIXEL_BITS: u32 = 16;

/// `2 ** SUBPIXEL_BITS`. Multiply an `f64` pixel value by this to get its
/// `i64` sub-pixel representation; divide an `i64` sub-pixel value by this to
/// get back `f64` CSS pixels.
pub(crate) const SUBPIXEL_FACTOR: i64 = 1 << SUBPIXEL_BITS;

/// Convert an `f64` measurement into `i64` sub-pixels. Clamps negative or NaN
/// inputs to zero, and saturates anything past `i64::MAX` (including
/// `+infinity`) so a malformed input cannot poison the prefix-sum.
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

