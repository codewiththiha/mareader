//! Applies the persisted appearance to the DOM whenever it changes.
//!
//! Base mode -> `<html data-base="light|dark|dim">` (+ `.dark` class).
//! Tint      -> computed `--canvas-filter` / `--canvas-blend` + seven UI token
//!              overrides, all written as inline custom properties on `<html>`.
//! Texture   -> `--texture-opacity` / `--texture-scale-user`.
//! Noise     -> `body.noise-enabled` (+ `.noise-animated`) and `--noise-opacity`.
//!
//! WHY INLINE PROPERTIES RATHER THAN CSS BLOCKS. The old design had one
//! `:root[data-theme=...]` block per theme, so every look needed hand-written
//! CSS and only the six that existed were reachable. The tint is now continuous
//! — any hue, any strength — which cannot be enumerated in a stylesheet. The
//! stylesheet keeps the STRUCTURE (which var drives what) and the base
//! palettes; the computed values are pushed here. Setting a property to the
//! empty string removes the override and lets the stylesheet's own value win
//! again, which is how a tint is cleanly un-applied.
//!
//! SLIDER RAM (appendix 19). Writing `settings` on every `input` event made
//! WKWebView allocate a fresh filter intermediate for every visible page,
//! every tick — that is the 1.2GB spike while dragging Colour / Tint
//! strength. Sliders now live-paint CSS at most once per animation frame and
//! commit the Settings signal (and localStorage) only after the gesture
//! pauses. The filter STRING is unchanged, so the look is byte-identical.

use std::cell::{Cell, RefCell};
use std::time::Duration;

use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use crate::core::appearance::Appearance;
use crate::core::settings::Settings;
use crate::core::state::AppState;
use crate::util::storage::save_settings;

/// The seven tokens the tint may override. Listed once so they can be cleared
/// as a set — a stale override left behind when the tint is removed would keep
/// tinting the UI with no way for the user to see why.
const UI_TOKENS: [&str; 7] = [
    "--color-paper",
    "--color-surface",
    "--color-line",
    "--color-ink",
    "--color-muted",
    "--color-accent",
    "--color-accent-soft",
];

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
    static PAINT_PENDING: Cell<Option<Appearance>> = const { Cell::new(None) };
    static PAINT_SCHEDULED: Cell<bool> = const { Cell::new(false) };
    static COMMIT_GEN: Cell<u64> = const { Cell::new(0) };
    static COMMIT_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
    static COMMIT_PAYLOAD: Cell<Option<(AppState, AppearanceScrub)>> = const { Cell::new(None) };
    static SAVE_TIMER: RefCell<Option<TimeoutHandle>> = const { RefCell::new(None) };
}

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
}

fn html_style() -> Option<web_sys::CssStyleDeclaration> {
    document_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|h| h.style())
}

fn body_el() -> Option<web_sys::HtmlElement> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
}

fn set_scrubbing(on: bool) {
    let Some(body) = body_el() else { return };
    let class = body.class_list();
    if on {
        let _ = class.add_1("appearance-scrubbing");
    } else {
        let _ = class.remove_1("appearance-scrubbing");
    }
}

/// Drop the frosted-header backdrop snapshot for one reflow.
///
/// WKWebView caches `backdrop-filter` against the pixels that were
/// behind the toolbar. Changing `--color-surface` / `data-base` updates
/// the PAGE immediately, but the glass layer keeps compositing the OLD
/// dark page under the NEW light surface — the toolbar sits at a mid-mix
/// until something (unmounting the appearance popover) rebuilds the
/// layer. Forcing `backdrop-filter: none` through a reflow makes it
/// re-sample the page that is actually on screen.
fn kick_backdrop_surfaces() {
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for sel in [".toolbar-glass", ".floating-search-enter"] {
        let Some(el) = doc.query_selector(sel).ok().flatten() else {
            continue;
        };
        let Ok(html) = el.dyn_into::<web_sys::HtmlElement>() else {
            continue;
        };
        let style = html.style();
        let _ = style.set_property("-webkit-backdrop-filter", "none");
        let _ = style.set_property("backdrop-filter", "none");
        let _ = html.offset_height();
        let _ = style.remove_property("-webkit-backdrop-filter");
        let _ = style.remove_property("backdrop-filter");
    }
}

/// Write every appearance CSS custom property / class from `a`. Synchronous.
/// The filter string is the same one `Appearance::canvas_filter` already
/// produces — this does not invent a second pipeline.
pub fn paint_appearance_now(a: Appearance) {
    let Some(el) = document_element() else { return };

    let prev_base = el.get_attribute("data-base");
    // Only a Light/Dark/Dim swap needs the glass layer rebuilt. Slider
    // ticks must not (appendix 19), and a same-base tint is already live
    // on `--color-*` — `.toolbar-glass:has(.menu-popover)` drops the
    // stale backdrop while the picker is open.
    let kick = prev_base.as_deref() != Some(a.base.as_str());

    let _ = el.set_attribute("data-base", a.base.as_str());
    let class = el.class_list();
    if a.base.is_dark() {
        let _ = class.add_1("dark");
    } else {
        let _ = class.remove_1("dark");
    }
    if kick {
        // Kill color transitions for this frame so toolbar buttons cannot
        // linger at a mid-mix of the old and new tokens.
        let _ = class.add_1("theme-switching");
    }

    if let Some(style) = html_style() {
        let _ = style.set_property(
            "color-scheme",
            if a.base.is_dark() { "dark" } else { "light" },
        );
        let _ = style.set_property("--canvas-filter", &a.canvas_filter());
        let _ = style.set_property("--canvas-blend", a.canvas_blend());
        for tok in UI_TOKENS {
            let _ = style.remove_property(tok);
        }
        for (name, value) in a.ui_overrides() {
            let _ = style.set_property(name, &value);
        }
        let _ = style.set_property(
            "--texture-opacity",
            &format!("{:.3}", a.texture_opacity as f64 / 100.0),
        );
        let _ = style.set_property(
            "--texture-scale-user",
            &format!("{:.3}", a.texture_scale as f64 / 100.0),
        );
    }

    if kick {
        kick_backdrop_surfaces();
        request_animation_frame(move || {
            if let Some(el) = document_element() {
                let _ = el.class_list().remove_1("theme-switching");
            }
        });
    }

    let Some(body) = body_el() else { return };
    let class = body.class_list();
    if a.noise.is_on() {
        let _ = class.add_1("noise-enabled");
    } else {
        let _ = class.remove_1("noise-enabled");
    }
    if matches!(a.noise, crate::core::appearance::NoiseMode::Animated) {
        let _ = class.add_1("noise-animated");
    } else {
        let _ = class.remove_1("noise-animated");
    }
    let _ = body.style().set_property(
        "--noise-opacity",
        &format!("{}", a.noise_intensity.min(100) as f64 / 100.0),
    );
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
    set_scrubbing(false);
}

/// Apply a pending slider commit NOW, then clear the timer. Used when a
/// structural click (base / texture mode / grain mode) should keep the
/// hue the reader just dialled.
pub fn flush_appearance_commit() {
    clear_commit_timer();
    let payload = COMMIT_PAYLOAD.with(|p| p.take());
    bump_commit_gen();
    set_scrubbing(false);
    if let Some((state, patch)) = payload {
        state.settings.update(|s| {
            apply_scrub(&mut s.appearance, patch);
            s.touch_appearance();
        });
    }
}

/// Live-preview a slider: paint CSS this frame, write Settings once the
/// gesture pauses. Does NOT notify `settings` on the way, so PageCanvas /
/// presets / localStorage stay quiet for the whole drag.
pub fn preview_appearance(state: AppState, patch: AppearanceScrub) {
    let mut a = state.settings.get_untracked().appearance;
    apply_scrub(&mut a, patch);
    set_scrubbing(true);
    paint_appearance(a);

    let gen = bump_commit_gen();
    COMMIT_PAYLOAD.with(|p| p.set(Some((state, patch))));
    clear_commit_timer();
    let handle = set_timeout_with_handle(
        move || {
            if COMMIT_GEN.with(|g| g.get()) != gen {
                return;
            }
            COMMIT_PAYLOAD.with(|p| p.set(None));
            set_scrubbing(false);
            state.settings.update(|s| {
                apply_scrub(&mut s.appearance, patch);
                s.touch_appearance();
            });
        },
        Duration::from_millis(COMMIT_MS),
    )
    .ok();
    COMMIT_TIMER.with(|t| *t.borrow_mut() = handle);
}

fn schedule_save(settings: Settings) {
    if let Some(h) = SAVE_TIMER.with(|t| t.borrow_mut().take()) {
        h.clear();
    }
    let handle = set_timeout_with_handle(
        move || {
            save_settings(&settings);
        },
        Duration::from_millis(SAVE_MS),
    )
    .ok();
    SAVE_TIMER.with(|t| *t.borrow_mut() = handle);
}

pub fn theme_applier(state: AppState) {
    // One effect, one paint: hue / texture / grain all live on Appearance,
    // and the live-preview path writes the same properties, so splitting
    // them into three effects just tripled the work on every settings write.
    Effect::new(move || {
        let a = state.settings.get().appearance;
        paint_appearance_now(a);
    });

    Effect::new(move || {
        let settings = state.settings.get();
        schedule_save(settings);
    });
}
