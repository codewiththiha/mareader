//! Appearance slider scrub + commit scheduler, split out of `theme.rs`.
//!
//! Sliders live-paint CSS at most once per animation frame (never per `input`
//! event — the original per-event painting drove a 1.2GB WKWebView spike
//! that motivated it) and commit the Settings signal + localStorage only after
//! the gesture pauses. Structural clicks (preset, base, texture mode, grain
//! mode) flush or cancel a pending scrub through `flush_appearance_commit` /
//! `cancel_appearance_commit`.
//!
//! Pipeline-specific hooks live in the siblings: `raster` owns the engine
//! bridge (re-bake / scrub mode — only the raster pipeline needs it) and
//! `reflow` recomputes the reflowable page's tokens, which repaint from CSS
//! alone.

pub mod raster;
pub mod reflow;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use leptos::prelude::*;

use reader_core::appearance::Appearance;
use reader_core::settings::Settings;
use storage::save_settings;

use crate::theme_paint::paint_into;

/// The live paint's destination — re-exported for the routed handle's
/// implementation (the reader host's theme module).
pub use crate::theme_paint::PaintTarget;

/// The slider patch vocabulary is shared with the theme handle's scrub
/// callback; re-exported for the menu's dial call sites.
pub use reader_core::appearance::AppearanceScrub;

/// Where a theme edit goes. `Routed` follows the app's routing rule: while
/// independent themes are on it edits the ACTIVE pane's own look; while they
/// are off it edits the window's theme like any other settings change.
/// `Global` always edits the window's theme — the film grain (noise) dial is
/// the one dial that stays global.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeScope {
    Routed,
    Global,
}

/// A structural or preset edit: whatever it needs of the appearance, applied
/// in place (the caller's closure owns the semantics).
pub type AppearancePatch = Box<dyn FnOnce(&mut Appearance)>;

/// The menu's one door into whichever theme it is editing. The reader host
/// builds a routed handle (its pane looks live behind it); a surface with no
/// workspace — the shelf — uses [`ThemeHandle::for_settings`], where both
/// scopes land in Settings. The dials read `look` (the edit target's current
/// values) and write through `commit` / `scrub`; `independent` + the theme
/// toggle ride along so the menu can show and flip it.
#[derive(Clone, Copy)]
pub struct ThemeHandle {
    /// The look the dials currently edit and display.
    pub look: Signal<Appearance>,
    pub independent: Signal<bool>,
    pub set_independent: Callback<bool>,
    pub commit: Callback<(ThemeScope, AppearancePatch)>,
    pub scrub: Callback<(ThemeScope, AppearanceScrub)>,
}

impl ThemeHandle {
    /// The window-theme handle: every edit is a Settings edit.
    pub fn for_settings(settings: RwSignal<Settings>) -> Self {
        let look = Signal::derive(move || settings.with(|s| s.appearance));
        let commit = Callback::new(move |(scope, patch): (ThemeScope, AppearancePatch)| {
            let _ = scope;
            flush_appearance_commit();
            settings.update(|s| {
                patch(&mut s.appearance);
                s.touch_appearance();
            });
        });
        let scrub = Callback::new(move |(scope, patch): (ThemeScope, AppearanceScrub)| {
            let _ = scope;
            preview_appearance(settings, patch);
        });
        Self {
            look,
            independent: Signal::derive(|| false),
            set_independent: Callback::new(|_| {}),
            commit,
            scrub,
        }
    }
}

/// How long after the last slider tick we write Settings. Long enough that a
/// continuous drag is one write; short enough that a tap still feels instant.
const COMMIT_MS: u64 = 180;
/// Persist can wait a beat — last_path and a finished drag both settle here.
const SAVE_MS: u64 = 350;

fn apply_scrub(a: &mut Appearance, p: AppearanceScrub) {
    p.apply(a);
}

/// A slider gesture's settled commit: the patch it landed on, and the sink
/// that writes it wherever the gesture is editing (Settings, or a pane's
/// look).
type CommitPayload = (AppearanceScrub, Box<dyn FnOnce(AppearanceScrub)>);

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
        raster::set_scrub_mode(true);
    }
}

fn leave_scrub() {
    let was = SCRUBBING.with(|s| s.replace(false));
    if was {
        raster::set_scrub_mode(false);
    }
}

/// True while an appearance slider scrub is in flight. The theme applier
/// consults this to leave the engine's rasters alone mid-gesture: the drag
/// has been repainting the variables every frame, and the scrub EXIT is what
/// performs the one final bake at the values the drag landed on.
pub fn is_scrubbing() -> bool {
    SCRUBBING.with(|s| s.get())
}

/// Whether the appearance popover is open. The engine's retention gate
/// treats an open menu as a scrub about to happen: pages that finish
/// rendering while the reader is looking at the dials keep their unbaked
/// rasters, so the FIRST drag of a session blits them under the live CSS
/// instead of re-rendering every page. The bridge guard drops the call when
/// no engine is mounted; a reflowable document simply has no pages to
/// retain.
pub fn set_appearance_menu_open(on: bool) {
    raster::set_appearance_menu_open(on);
}

thread_local! {
    static PAINT_PENDING: Cell<Option<(Appearance, f64, PaintTarget)>> = const { Cell::new(None) };
    static PAINT_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static COMMIT_GEN: Cell<u64> = const { Cell::new(0) };
    static COMMIT_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
    static COMMIT_PAYLOAD: RefCell<Option<CommitPayload>> = const { RefCell::new(None) };
    static SAVE_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
}

/// Coalesce paints onto the next animation frame. A 60Hz slider would
/// otherwise rewrite `--canvas-filter` more than once per composite, and
/// each rewrite is a new WKWebView filter intermediate per visible page.
/// The ink dial rides along so the text tokens repaint from the same
/// snapshot the scrub is dialling.
fn paint_appearance(a: Appearance, ink_contrast: f64, target: PaintTarget) {
    PAINT_PENDING.with(|p| p.set(Some((a, ink_contrast, target))));
    if PAINT_SCHEDULED.with(|s| s.get()) {
        return;
    }
    PAINT_SCHEDULED.with(|s| s.set(true));
    request_animation_frame(move || {
        PAINT_SCHEDULED.with(|s| s.set(false));
        if let Some((a, ic, target)) = PAINT_PENDING.with(|p| p.take()) {
            paint_into(target, a, ic);
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
    COMMIT_PAYLOAD.with(|p| *p.borrow_mut() = None);
    // The scrub is over even though its timer never fired; restore baked
    // rasters at whatever the variables currently hold.
    leave_scrub();
}

/// Apply a pending slider commit NOW, then clear the timer. Used when a
/// structural click (base / texture mode / grain mode) should keep the
/// hue the reader just dialled.
pub fn flush_appearance_commit() {
    clear_commit_timer();
    let payload = COMMIT_PAYLOAD.with(|p| p.borrow_mut().take());
    bump_commit_gen();
    if let Some((patch, sink)) = payload {
        // Update final values first: the theme effect queues its refresh at
        // those values, then leaving scrub queues behind it and cannot cause a
        // stale first rebake followed by a second one. The sink owns where
        // the settled values land (Settings or a pane's look).
        sink(patch);
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

/// Live-preview a slider: paint CSS this frame, run the commit sink once the
/// gesture pauses. The source `current` is what the dial edits (Settings'
/// appearance, or a pane's look), `target` where the live paint lands, and
/// `sink` where the settled values are written. Does NOT notify any signal
/// on the way, so PageCanvas / presets / localStorage stay quiet for the
/// whole drag.
pub fn preview_appearance_into(
    current: Appearance,
    ink_contrast: f64,
    target: PaintTarget,
    patch: AppearanceScrub,
    sink: Box<dyn FnOnce(AppearanceScrub)>,
) {
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

    let mut a = current;
    apply_scrub(&mut a, patch);
    paint_appearance(a, ink_contrast, target);
    // The page re-colours under the drag through the live CSS pipeline
    // alone — that is what scrub mode exists for. The engine is deliberately
    // NOT told per tick: the bridge crossing (and the serialized no-op it
    // queues while scrub owns the canvases) is main-thread work in the middle
    // of the frame budget the drag is trying to hit, once per input event for
    // the length of the gesture. The scrub exit performs the single final
    // bake at the values the drag settled on; a text document never hears
    // about any of it, its tokens having repainted from CSS all along.

    let commit_gen = bump_commit_gen();
    COMMIT_PAYLOAD.with(|p| *p.borrow_mut() = Some((patch, sink)));
    clear_commit_timer();
    let handle = set_timeout_with_handle(
        move || {
            if COMMIT_GEN.with(|g| g.get()) != commit_gen {
                return;
            }
            let payload = COMMIT_PAYLOAD.with(|p| p.borrow_mut().take());
            // End the gesture before committing it. Leaving scrub clears the
            // flag and queues the one final bake at the settled values; the
            // settings write that follows wakes the theme effect OUTSIDE that
            // window instead of inside it. The commit used to land first, so
            // the effect the write woke raced the scrub flag on its way down
            // and its engine refresh crossed the bridge while the gesture's
            // own exit bake was still in flight. Now whatever the effect
            // queues serializes behind the exit on the engine's theme queue
            // and converges there as a same-fingerprint no-op: one bake per
            // drag, not one per tick and not two at the end.
            if patch_needs_canvas_scrub(patch) {
                leave_scrub();
            }
            if let Some((patch, sink)) = payload {
                sink(patch);
            }
        },
        Duration::from_millis(COMMIT_MS),
    )
    .ok();
    COMMIT_TIMER.with(|t| *t.borrow_mut() = handle);
}

/// The window/Settings case of [`preview_appearance_into`]: the historical
/// slider entry (and the shelf handle's scrub sink).
pub fn preview_appearance(settings: RwSignal<Settings>, patch: AppearanceScrub) {
    let current = settings.get_untracked().appearance;
    let ink_contrast = settings.get_untracked().text.ink_contrast;
    preview_appearance_into(
        current,
        ink_contrast,
        PaintTarget::Window,
        patch,
        Box::new(move |p| {
            settings.update(|s| {
                apply_scrub(&mut s.appearance, p);
                s.touch_appearance();
            });
        }),
    );
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
