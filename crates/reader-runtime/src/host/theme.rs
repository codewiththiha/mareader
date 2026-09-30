//! The workspace's independent theme state: one look per pane, toggled
//! from the appearance menu while a split workspace is on screen.
//!
//! Routing rule (the walkthrough's): while independent themes are ON, every
//! appearance edit — structural or slider — routes to the ACTIVE pane's own
//! look, even with one pane ("one per one open keeps green"). While they are
//! OFF, edits are ordinary Settings edits. The film grain dial is the one
//! dial that stays global whatever the toggle says. The shared chrome (title
//! bar, sidebar, the backdrop outside panes) keeps the remembered global
//! theme throughout; only a pane's own box shows its own look.
//!
//! Lifetime rules, from the close/fallback walkthrough (4 panes
//! blue/red/yellow/green): the working colour is the ACTIVE pane's colour —
//! a non-active close never moves focus, an active close hands it to the
//! tree's successor — so no separate "last activated" tracker exists. An
//! override survives in-place document opens (it is keyed by the PANE id,
//! which a replace keeps) and dies with its pane. On split exit the last
//! active pane's colour is simply that surviving pane's override: it drives
//! later single-pane opens and new splits (a new pane seeds from the active
//! pane's look). The global theme is remembered untouched while the toggle
//! is on; turning it off clears every override and the panes inherit again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use leptos::prelude::*;
use reader_core::appearance::Appearance;
use reader_core::settings::Settings;

use super::model::PaneId;

/// One workspace's per-pane looks. Copy: views and callbacks capture it.
#[derive(Clone, Copy)]
pub struct PaneThemes {
    /// The remembered toggle. Persisted with the workspace settings by the
    /// shell; the host only reads and publishes it.
    independent: RwSignal<bool>,
    /// Bumped on every map or toggle change: the host's appearance boundary
    /// tracks it and re-pushes each pane's effective look.
    version: RwSignal<u64>,
    overrides: StoredValue<Rc<RefCell<HashMap<PaneId, Appearance>>>, LocalStorage>,
}

impl PaneThemes {
    /// Themes for one workspace. Call inside the host's owner (the signals
    /// live in the host's arena).
    pub fn new(independent: RwSignal<bool>) -> Self {
        Self {
            independent,
            version: RwSignal::new(0),
            overrides: StoredValue::new_local(Rc::new(RefCell::new(HashMap::new()))),
        }
    }

    pub fn independent(self) -> Signal<bool> {
        self.independent.into()
    }

    /// The change token the appearance boundary tracks alongside settings.
    pub fn version(self) -> RwSignal<u64> {
        self.version
    }

    fn bump(self) {
        self.version.update(|v| *v = v.wrapping_add(1));
    }

    /// The look this pane owns while independent themes are on (`None` =
    /// inherit the window theme). A pane missing from the map answers the
    /// defensive `global` — seeding keeps that arm unreachable in practice.
    pub fn look_for(self, id: PaneId, global: Appearance) -> Option<Appearance> {
        if !self.independent.get_untracked() {
            return None;
        }
        Some(
            self.overrides
                .with_value(|m| m.borrow().get(&id).copied())
                .unwrap_or(global),
        )
    }

    /// The active pane's current look — what the menu's dials edit and show
    /// while independent themes are on.
    pub fn active_look(self, active: Option<PaneId>, global: Appearance) -> Appearance {
        active
            .and_then(|id| self.overrides.with_value(|m| m.borrow().get(&id).copied()))
            .unwrap_or(global)
    }

    /// Seed (or overwrite) one pane's look. The toggle-on seeds every placed
    /// pane from the global look; a pane created while the toggle is on
    /// seeds from the ACTIVE pane's look instead (the caller decides).
    pub fn seed(self, id: PaneId, look: Appearance) {
        self.overrides.with_value(|m| {
            m.borrow_mut().insert(id, look);
        });
        self.bump();
    }

    /// A closed pane takes its look with it. The fallback colour is never
    /// lost: the surviving ACTIVE pane's look is the working colour.
    pub fn forget(self, id: PaneId) {
        self.overrides.with_value(|m| {
            m.borrow_mut().remove(&id);
        });
        self.bump();
    }

    /// Turn the toggle on: every placed pane starts from the global look.
    /// The global theme itself is left untouched (remembered, not shown).
    pub fn enable(self, placed: impl Iterator<Item = PaneId>, global: Appearance) {
        self.overrides.with_value(|m| {
            let mut m = m.borrow_mut();
            for id in placed {
                m.insert(id, global);
            }
        });
        self.independent.set(true);
        self.bump();
    }

    /// Turn the toggle off: every override goes with it and the panes
    /// inherit the window theme again. What the panes painted as their own
    /// is removed by the pane paint on the next boundary push (`look: None`).
    pub fn disable(self) {
        self.overrides.with_value(|m| m.borrow_mut().clear());
        self.independent.set(false);
        self.bump();
    }

    /// Apply an edit to one pane's look (the routed commit).
    pub fn edit(self, id: PaneId, global: Appearance, patch: impl FnOnce(&mut Appearance)) {
        self.overrides.with_value(|m| {
            let mut m = m.borrow_mut();
            let mut look = m.get(&id).copied().unwrap_or(global);
            patch(&mut look);
            look.sanitize();
            m.insert(id, look);
        });
        self.bump();
    }

    /// A scrub tick's live paint: the pane's look at the dragged values,
    /// published to the pane root through the boundary (rAF-coalesced by the
    /// scheduler that called in). The map is written every tick — the values
    /// are exactly what the drag shows, so a settled commit has nothing left
    /// to land.
    pub fn preview(self, id: PaneId, look: Appearance) {
        self.overrides.with_value(|m| {
            m.borrow_mut().insert(id, look);
        });
        self.bump();
    }
}

/// Build the menu's theme handle for this workspace. The routed arms follow
/// the routing rule above; the global arms (and every edit while the toggle
/// is off) are plain Settings writes. `manager` supplies the active pane and
/// the placement list; `settings` is the session's settings signal.
pub(crate) fn theme_handle(
    themes: PaneThemes,
    manager: super::manager::PaneManager,
    settings: RwSignal<Settings>,
) -> app_ui::appearance::ThemeHandle {
    use app_ui::appearance::{
        PaintTarget, ThemeScope, preview_appearance, preview_appearance_into,
    };

    let look = Signal::derive(move || {
        let global = settings.with(|s| s.appearance);
        if themes.independent.get() {
            themes.active_look(manager.active(), global)
        } else {
            global
        }
    });

    let set_independent = Callback::new(move |on: bool| {
        // The toggle's rest state is a workspace setting; the per-pane
        // colours it carries are temporary and never persisted.
        settings.update(|s| s.workspace.independent_themes = on);
        if on {
            let global = settings.get_untracked().appearance;
            themes.enable(manager.placed().into_iter(), global);
        } else {
            themes.disable();
        }
    });

    let commit = Callback::new(
        move |(scope, patch): (ThemeScope, app_ui::appearance::AppearancePatch)| {
            use app_ui::appearance::flush_appearance_commit;
            // A pending slider drag lands first (the shared scheduler's
            // contract), then the structural change applies on top.
            flush_appearance_commit();
            let routed = scope == ThemeScope::Routed
                && themes.independent.get_untracked()
                && manager.active().is_some();
            if routed {
                let id = manager.active().expect("checked: active pane exists");
                let global = settings.get_untracked().appearance;
                themes.edit(id, global, patch);
            } else {
                settings.update(|s| {
                    patch(&mut s.appearance);
                    s.touch_appearance();
                });
            }
        },
    );

    let scrub = Callback::new(
        move |(scope, patch): (ThemeScope, reader_core::appearance::AppearanceScrub)| {
            let routed = scope == ThemeScope::Routed
                && themes.independent.get_untracked()
                && manager.active().is_some();
            if routed {
                let id = manager.active().expect("checked: active pane exists");
                let global = settings.get_untracked().appearance;
                let current = themes.active_look(manager.active(), global);
                let ink = settings.get_untracked().text.ink_contrast;
                preview_appearance_into(
                    current,
                    ink,
                    PaintTarget::Delegated(Box::new(move |a, _ink| {
                        themes.preview(id, a);
                    })),
                    patch,
                    // The tick paints already wrote the map at exactly these
                    // values; a settled commit has nothing left to land.
                    Box::new(|_p| {}),
                );
            } else {
                preview_appearance(settings, patch);
            }
        },
    );

    app_ui::appearance::ThemeHandle {
        look,
        independent: themes.independent(),
        set_independent,
        commit,
        scrub,
    }
}
