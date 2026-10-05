//! The app's one clock.
//!
//! Milliseconds since the epoch, which is the stamp on a book's join and its
//! last read, on a tombstone, on a folder's last scan, and the seed of every
//! id `library_core::id` mints. One function rather than one copy per module
//! that needs a stamp: a clock is exactly the kind of rule that cannot be
//! allowed to drift per caller.
//!
//! Off wasm the clock is inert rather than a panic: the wasm-bindgen stubs
//! abort when called natively, and a stamp nobody persists is fine at zero.
//! Ids minted from it stay unique regardless, on `library_core::id`'s own
//! counter — which is what lets the host tests import, restore and remove at
//! all.

/// Milliseconds since the Unix epoch; `0` off wasm (host tests).
pub fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        0
    }
}
