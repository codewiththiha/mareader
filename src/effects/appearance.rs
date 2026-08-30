//! Appearance slider scrub + commit scheduler, split out of `theme.rs`.
//!
//! Sliders live-paint CSS at most once per animation frame (never per `input`
//! event — the original per-event painting drove a 1.2GB WKWebView spike
//! that motivated it) and commit the Settings signal + localStorage only after
//! the gesture pauses. Structural clicks (preset, base, texture mode, grain
//! mode) flush or cancel a pending scrub through `flush_appearance_commit` /
//! `cancel_appearance_commit`.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use leptos::prelude::*;

use pdf_core::appearance::Appearance;
use pdf_core::settings::Settings;
use crate::storage::save_settings;

use crate::effects::app::theme::paint_appearance_now;

/// How long after the last slider tick we write Settings. Long enough that a
/// continuous drag is one write; short enough that a tap still feels instant.
const COMMIT_MS: u64 = 180;
/// Persist can wait a beat — last_path and a finished drag both settle here.
const SAVE_MS: u64 = 350;

/// One field a slider is allowed to live-edit. Structural clicks (preset,
/// base, texture mode, grain mode) go through `settings.update` directly
/// and must either flush or cancel a pending scrub first.
#[derive(Debug, Clone, Copy)]
pub enum AppearanceScrub {
    Tint { hue: u16, strength: u8 },
    TextureOpacity(u8),
    TextureScale(u16),
    NoiseIntensity(u8),
}

fn apply_scrub(a: &mut Appearance, p: AppearanceScrub) {
    match p {
        AppearanceScrub::Tint { hue, strength } => {
            a.tint_hue = hue;
            a.tint_strength = strength;
        }
        AppearanceScrub::TextureOpacity(v) => a.texture_opacity = v,
        AppearanceScrub::TextureScale(v) => a.texture_scale = v,
        AppearanceScrub::NoiseIntensity(v) => a.noise_intensity = v,
    }
    a.sanitize();
}

thread_local! {
    // True while an appearance slider scrub is in flight. The engine then
    // shows RAW rasters under the live CSS filter/blend (see
    // `engine::set_scrub_mode`) so the page re-colours every frame under the
    // user's drag — a per-frame re-bake of full-resolution rasters cannot
    // keep up, and the compositor's per-frame filter is exactly what the
    // pre-baking pipeline did.
    static SCRUBBING: Cell<bool> = const { Cell::new(false) };
}

fn enter_scrub() {
    let was = SCRUBBING.with(|s| s.replace(true));
    if !was {
        pdf_engine::api::set_scrub_mode(true);
    }
}

fn leave_scrub() {
    let was = SCRUBBING.with(|s| s.replace(false));
    if was {
        pdf_engine::api::set_scrub_mode(false);
    }
}

thread_local! {
    static PAINT_PENDING: Cell<Option<Appearance>> = const { Cell::new(None) };
    static PAINT_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static COMMIT_GEN: Cell<u64> = const { Cell::new(0) };
    static COMMIT_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
    static COMMIT_PAYLOAD: Cell<Option<(RwSignal<Settings>, AppearanceScrub)>> = const { Cell::new(None) };
    static SAVE_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
}

/// Coalesce paints onto the next animation frame. A 60Hz slider would
/// otherwise rewrite `--canvas-filter` more than once per composite, and
/// each rewrite is a new WKWebView filter intermediate per visible page.
fn paint_appearance(a: Appearance) {
    PAINT_PENDING.with(|p| p.set(Some(a)));
    if PAINT_SCHEDULED.with(|s| s.get()) {
        return;
    }
    PAINT_SCHEDULED.with(|s| s.set(true));
    request_animation_frame(move || {
        PAINT_SCHEDULED.with(|s| s.set(false));
        if let Some(a) = PAINT_PENDING.with(|p| p.take()) {
            paint_appearance_now(a);
        }
    });
}

fn bump_commit_gen() -> u64 {
    COMMIT_GEN.with(|g| {
        let n = g.get() + 1;
        g.set(n);
        n
    })
}

fn clear_commit_timer() {
    if let Some(h) = COMMIT_TIMER.with(|t| t.borrow_mut().take()) {
        h.clear();
    }
}

/// Drop a pending slider commit without writing Settings. Used when a
/// preset (or any other structural click) should win over an in-flight drag.
pub fn cancel_appearance_commit() {
    bump_commit_gen();
    clear_commit_timer();
    COMMIT_PAYLOAD.with(|p| p.set(None));
    // The scrub is over even though its timer never fired; restore baked
    // rasters at whatever the variables currently hold.
    leave_scrub();
}

/// Apply a pending slider commit NOW, then clear the timer. Used when a
/// structural click (base / texture mode / grain mode) should keep the
/// hue the reader just dialled.
pub fn flush_appearance_commit() {
    clear_commit_timer();
    let payload = COMMIT_PAYLOAD.with(|p| p.take());
    bump_commit_gen();
    if let Some((settings, patch)) = payload {
        // Update final values first: the theme effect queues its refresh at
        // those values, then leaving scrub queues behind it and cannot cause a
        // stale first rebake followed by a second one.
        settings.update(|s| {
            apply_scrub(&mut s.appearance, patch);
            s.touch_appearance();
        });
    }
    // A flush ends the gesture whatever it was scrubbing. Gating this on
    // `patch_needs_canvas_scrub` left the engine in scrub mode when a tint
    // drag was superseded by a texture drag before the flush, which pins
    // every page's raw canvas in memory. `leave_scrub` is a no-op when no
    // scrub is active, so calling it unconditionally is safe.
    leave_scrub();
}

fn patch_needs_canvas_scrub(p: AppearanceScrub) -> bool {
    // Only tint rewrites `--canvas-filter` / `--canvas-blend`. Noise and
    // texture sliders only touch overlays; putting the engine in scrub mode
    // would apply those CSS filters on already-baked pixels (Dark flashes
    // to light, Dim goes darker) for no reason.
    matches!(p, AppearanceScrub::Tint { .. })
}

/// Live-preview a slider: paint CSS this frame, write Settings once the
/// gesture pauses. Does NOT notify `settings` on the way, so PageCanvas /
/// presets / localStorage stay quiet for the whole drag.
pub fn preview_appearance(settings: RwSignal<Settings>, patch: AppearanceScrub) {
    // The theme variables change every frame from here on; switch the engine
    // to raw rasters + live CSS so the PAGE tracks a tint drag. Overlay
    // sliders (noise / texture) must not enter canvas scrub.
    if patch_needs_canvas_scrub(patch) {
        enter_scrub();
    } else {
        // Moving from a tint scrub straight onto a non-canvas slider
        // (texture / noise) still ENDS the canvas scrub. Without this the
        // tint's own commit timer is invalidated by `bump_commit_gen`
        // below, returns early, and never calls `leave_scrub` — while the
        // texture timer never calls it either. The engine's
        // `themeScrubActive` then stays true forever, `dropRawIfIdle`
        // bails on every page, and each one holds BOTH its raw and its
        // baked canvas for the rest of the session (the ~900MB plateau).
        leave_scrub();
    }

    let mut a = settings.get_untracked().appearance;
    apply_scrub(&mut a, patch);
    paint_appearance(a);
    // The page re-colours under the drag through the live CSS pipeline.
    // refresh_theme keeps the engine's pipeline cache honest for the rebake
    // (a no-op while scrub owns the canvases); the blend backdrop follows in
    // the same frame on its own, being pure CSS over the same variables.
    pdf_engine::api::refresh_theme();

    let commit_gen = bump_commit_gen();
    COMMIT_PAYLOAD.with(|p| p.set(Some((settings, patch))));
    clear_commit_timer();
    let handle = set_timeout_with_handle(
        move || {
            if COMMIT_GEN.with(|g| g.get()) != commit_gen {
                return;
            }
            COMMIT_PAYLOAD.with(|p| p.set(None));
            // Commit final variables first so its refresh queues before the
            // scrub exit. The serialized engine then rebakes once at the
            // final values and only afterwards drops live CSS.
            settings.update(|s| {
                apply_scrub(&mut s.appearance, patch);
                s.touch_appearance();
            });
            if patch_needs_canvas_scrub(patch) {
                leave_scrub();
            }
        },
        Duration::from_millis(COMMIT_MS),
    )
    .ok();
    COMMIT_TIMER.with(|t| *t.borrow_mut() = handle);
}

/// Debounced Settings persistence. The apply_theme save effect calls this
/// on every settings change; a continuous drag settling into a single write
/// means one save, not one per tick.
pub fn schedule_save(settings: Settings) {
    if let Some(h) = SAVE_TIMER.with(|t| t.borrow_mut().take()) {
        h.clear();
    }
    let handle = set_timeout_with_handle(
        move || {
            if let Err(e) = save_settings(&settings) {
                e.report();
            }
        },
        Duration::from_millis(SAVE_MS),
    )
    .ok();
    SAVE_TIMER.with(|t| *t.borrow_mut() = handle);
}
