//! The appearance menu's downstream raster hooks, owned by whoever hosts a
//! raster pipeline — never by the menu itself.
//!
//! The appearance surfaces (sliders, presets, the popover lifetime) are
//! shared chrome: the library shows the same menu the reader does. But three
//! of the menu's side effects address a live raster engine — re-baking the
//! theme into mounted pages, the scrub window's raw-raster mode, and the
//! "menu is open, keep the unbaked raws" hint — and only the runtime that
//! OWNS an engine may answer them. Wiring those calls into the shared chrome
//! would put the PDF engine in every runtime's dependency graph, library
//! included.
//!
//! So the direction is inverted: a runtime that hosts an engine installs the
//! hooks for the length of its session, and the chrome calls the slot. No
//! installed hooks is a well-defined state — the menu still drives every CSS
//! variable it owns; the raster follow-ups are what a library session has no
//! business doing.

use std::cell::RefCell;
use std::rc::Rc;

/// What a raster engine owes the appearance surfaces. Implemented per
/// runtime that owns one; today that is the reader's PDF session.
pub trait AppearanceEngineHooks {
    /// Re-bake the theme into every raster the engine already holds.
    fn refresh_theme(&self);
    /// Enter/leave the real-time compositing used while a slider drags.
    fn set_scrub_mode(&self, on: bool);
    /// Whether the appearance popover is open (the engine retains the
    /// unbaked raws while it is).
    fn set_appearance_menu_open(&self, on: bool);
}

thread_local! {
    static HOOKS: RefCell<Option<Rc<dyn AppearanceEngineHooks>>> =
        const { RefCell::new(None) };
}

/// Install the engine hooks for the lifetime of one runtime session. The
/// caller drops the returned guard on teardown, which is what makes a closed
/// reader's hooks unreachable rather than silently stale.
pub fn install(hooks: Rc<dyn AppearanceEngineHooks>) -> AppearanceHooksGuard {
    HOOKS.with(|slot| *slot.borrow_mut() = Some(hooks));
    AppearanceHooksGuard
}

/// Removes the installed hooks on drop. Held by the session's cleanup chain.
pub struct AppearanceHooksGuard;

impl Drop for AppearanceHooksGuard {
    fn drop(&mut self) {
        HOOKS.with(|slot| *slot.borrow_mut() = None);
    }
}

fn with_hooks(f: impl FnOnce(&dyn AppearanceEngineHooks)) {
    HOOKS.with(|slot| {
        if let Some(hooks) = slot.borrow().as_ref() {
            f(hooks.as_ref());
        }
    });
}

/// Re-bake the theme into the host runtime's rasters, if one is installed.
pub fn refresh_theme() {
    with_hooks(|hooks| hooks.refresh_theme());
}

/// Enter/leave scrub mode on the host runtime's engine, if one is installed.
pub fn set_scrub_mode(on: bool) {
    with_hooks(|hooks| hooks.set_scrub_mode(on));
}

/// Tell the host runtime's engine whether the appearance menu is open, if
/// one is installed.
pub fn set_appearance_menu_open(on: bool) {
    with_hooks(|hooks| hooks.set_appearance_menu_open(on));
}
