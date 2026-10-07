//! Slider scrub and commit scheduling: paint per frame, commit on pause.

pub mod raster;
pub mod reflow;

use std::cell::{Cell, RefCell};
use std::time::Duration;

use leptos::prelude::*;

use reader_core::appearance::Appearance;
use reader_core::settings::Settings;
use storage::save_settings;

use crate::theme_paint::paint_into;

/// The live paint's destination, re-exported for the routed handle.
pub use crate::theme_paint::PaintTarget;

/// The slider patch vocabulary, shared with the theme handle's scrub.
pub use reader_core::appearance::AppearanceScrub;

/// Which family an edit belongs to, and so which preference scopes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeScope {
    /// Base mode and tint: routed while independent themes are in effect.
    Colour,
    /// Texture mode and its dials: routed while themes or textures split.
    Texture,
    Global,
}

/// A structural or preset edit, applied in place.
pub type AppearancePatch = Box<dyn FnOnce(&mut Appearance)>;

/// The menu's one door into whichever theme it edits.
#[derive(Clone, Copy)]
pub struct ThemeHandle {
    /// The look the dials currently edit and display.
    pub look: Signal<Appearance>,
    /// The STORED preference the switch shows and flips.
    pub independent: Signal<bool>,
    pub set_independent: Callback<bool>,
    /// The texture family's own per-pane mode, named the same way.
    pub independent_texture: Signal<bool>,
    pub set_independent_texture: Callback<bool>,
    pub commit: Callback<(ThemeScope, AppearancePatch)>,
    pub scrub: Callback<(ThemeScope, AppearanceScrub)>,
    /// How many panes the workspace shows; the toggle needs a split.
    pub panes: Signal<usize>,
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
        let set_independent = Callback::new(move |on: bool| {
            settings.update(|s| s.workspace.independent_themes = on);
        });
        let independent = Signal::derive(move || settings.with(|s| s.workspace.independent_themes));
        let set_independent_texture = Callback::new(move |on: bool| {
            settings.update(|s| s.workspace.independent_textures = on);
        });
        let independent_texture =
            Signal::derive(move || settings.with(|s| s.workspace.independent_textures));
        Self {
            look,
            independent,
            set_independent,
            independent_texture,
            set_independent_texture,
            commit,
            scrub,
            panes: Signal::derive(|| 1),
        }
    }
}

/// How long after the last slider tick Settings is written.
const COMMIT_MS: u64 = 180;
/// Persist can wait a beat — last_path and a finished drag both settle here.
const SAVE_MS: u64 = 350;

fn apply_scrub(a: &mut Appearance, p: AppearanceScrub) {
    p.apply(a);
}

/// A slider gesture's settled commit: the patch and its sink.
type CommitPayload = (AppearanceScrub, Box<dyn FnOnce(AppearanceScrub)>);

thread_local! {
    // True while an appearance slider scrub is in flight.
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

/// True while an appearance slider scrub is in flight.
pub fn is_scrubbing() -> bool {
    SCRUBBING.with(|s| s.get())
}

/// Whether the appearance popover is open: the engine's retention gate.
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

/// Coalesce paints onto the next animation frame.
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

/// Drop a pending slider commit without writing Settings.
pub fn cancel_appearance_commit() {
    bump_commit_gen();
    clear_commit_timer();
    COMMIT_PAYLOAD.with(|p| *p.borrow_mut() = None);
    // The scrub is over though its timer never fired: restore baked rasters.
    leave_scrub();
}

/// Apply a pending slider commit NOW, then clear the timer.
pub fn flush_appearance_commit() {
    clear_commit_timer();
    let payload = COMMIT_PAYLOAD.with(|p| p.borrow_mut().take());
    bump_commit_gen();
    if let Some((patch, sink)) = payload {
        // Update final values first, so the effect queues at those values.
        sink(patch);
    }
    // A flush ends the gesture, whatever it was scrubbing.
    leave_scrub();
}

fn patch_needs_canvas_scrub(p: AppearanceScrub) -> bool {
    // Only tint rewrites `--canvas-filter` / `--canvas-blend`.
    matches!(p, AppearanceScrub::Tint { .. })
}

/// Live-preview a slider: paint this frame, commit once the gesture pauses.
pub fn preview_appearance_into(
    current: Appearance,
    ink_contrast: f64,
    target: PaintTarget,
    patch: AppearanceScrub,
    sink: Box<dyn FnOnce(AppearanceScrub)>,
) {
    // From here the variables change per frame: switch the engine to raw.
    if patch_needs_canvas_scrub(patch) {
        enter_scrub();
    } else {
        // Moving from a tint scrub onto a non-canvas slider ends it too.
        leave_scrub();
    }

    let mut a = current;
    apply_scrub(&mut a, patch);
    paint_appearance(a, ink_contrast, target);
    // The page re-colours from live CSS alone; the scrub exit bakes once.

    let commit_gen = bump_commit_gen();
    COMMIT_PAYLOAD.with(|p| *p.borrow_mut() = Some((patch, sink)));
    clear_commit_timer();
    let handle = set_timeout_with_handle(
        move || {
            if COMMIT_GEN.with(|g| g.get()) != commit_gen {
                return;
            }
            let payload = COMMIT_PAYLOAD.with(|p| p.borrow_mut().take());
            // End the gesture first, so the commit's bake is one per drag.
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

/// Debounced Settings persistence.
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
