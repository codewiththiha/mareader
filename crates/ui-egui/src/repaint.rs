//! The window's side of [`mareader_engine::repaint::Repaint`].
//!
//! A controller below the front end asks for a frame without naming a toolkit;
//! this is the one place that answers with egui's. Built once per window and
//! cloned to whoever needs it, as `app/frame.rs` already does for the session's
//! `Waker`.

use std::sync::Arc;
use std::time::Duration;

use egui::Context;
use mareader_engine::repaint::Repaint;
use mareader_engine::session::Waker;

/// Answers a worker's ask with a frame from `ctx`.
pub fn wake(ctx: &Context) -> Waker {
    let ctx = ctx.clone();
    Arc::new(move || ctx.request_repaint())
}

/// Answers a controller's ask with a frame from `ctx`.
pub fn of(ctx: &Context) -> Repaint {
    let ctx = ctx.clone();
    Repaint::new(Arc::new(move |after: Duration| match after.is_zero() {
        true => ctx.request_repaint(),
        false => ctx.request_repaint_after(after),
    }))
}
