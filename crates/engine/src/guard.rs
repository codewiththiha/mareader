//! Panic firewall for engine entry points.
//!
//! Shipped engine code denies panicking helpers; this module is the last line
//! of defence so an unexpected panic becomes an error instead of crashing the
//! host window or headless client. On `wasm32`, panics abort and this is a
//! pass-through.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// Runs `f`, turning any unwinding panic into `Err(message)`.
pub fn catch_panic<T>(f: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(f)).map_err(|payload| panic_message(payload.as_ref()))
}

/// Extracts a readable message from a panic payload.
pub fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(msg) = payload.downcast_ref::<&str>() {
        (*msg).to_owned()
    } else if let Some(msg) = payload.downcast_ref::<String>() {
        msg.clone()
    } else {
        "unexpected engine panic".to_owned()
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn catch_panic_returns_value_or_message() {
        assert_eq!(catch_panic(|| 42), Ok(42));
        let static_err = catch_panic(|| -> i32 { panic!("static boom") });
        assert_eq!(static_err, Err("static boom".to_owned()));
        let owned_err = catch_panic(|| -> i32 { panic!("owned {}", 7) });
        assert_eq!(owned_err, Err("owned 7".to_owned()));
        let other_err = catch_panic(|| -> i32 { std::panic::panic_any(9_u8) });
        assert_eq!(other_err, Err("unexpected engine panic".to_owned()));
    }
}
