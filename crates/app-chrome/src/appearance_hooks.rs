//! The appearance menu's raster hooks, owned by whoever hosts an
//! engine.

use std::cell::RefCell;
use std::rc::Rc;

/// What a raster engine owes the appearance surfaces.
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

/// Install the engine hooks for one runtime session's lifetime.
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
