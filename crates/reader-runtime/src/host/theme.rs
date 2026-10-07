//! Per-pane theme state: each family routed to a pane while its mode is
//! in effect.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use leptos::prelude::*;
use reader_core::appearance::{Appearance, TextureMode};
use reader_core::settings::Settings;

use super::model::PaneId;

/// One workspace's per-pane looks. Copy: views and callbacks capture it.
#[derive(Clone, Copy)]
pub struct PaneThemes {
    /// The STORED preference, persisted by the shell; whether it is in
    /// effect is [`Self::active`]'s answer.
    independent: RwSignal<bool>,
    /// The texture family's own preference, with the same split gate.
    independent_textures: RwSignal<bool>,
    /// The preference AND the split it serves, derived once here.
    active: Signal<bool>,
    textures_active: Signal<bool>,
    /// The pane count the stand-down follows (the workspace's placement).
    panes: Signal<usize>,
    /// Bumped on every map or toggle change; the boundary tracks it.
    version: RwSignal<u64>,
    overrides: StoredValue<Rc<RefCell<HashMap<PaneId, Appearance>>>, LocalStorage>,
    /// Whether Light / Dark / Dim stays shared across panes (the setting).
    shared_base: Signal<bool>,
}

/// The tint strength a pane's own colour gets if it starts untinted.
const PANE_TINT_STRENGTH: u8 = 35;

/// A hue for a new pane: in the widest gap between `taken` hues.
fn distinct_hue(taken: &[u16], random: f64) -> u16 {
    let random = random.clamp(0.0, 1.0);
    if taken.is_empty() {
        return ((random * 360.0) as u16) % 360;
    }
    let mut hues: Vec<f64> = taken.iter().map(|h| f64::from(h % 360)).collect();
    hues.sort_by(f64::total_cmp);
    hues.dedup();
    let last = hues[hues.len() - 1];
    let (mut start, mut gap) = (last, hues[0] + 360.0 - last);
    for pair in hues.windows(2) {
        if pair[1] - pair[0] > gap {
            (start, gap) = (pair[0], pair[1] - pair[0]);
        }
    }
    // The middle half of the gap: never hugging a neighbour.
    let hue = start + gap * (0.25 + 0.5 * random);
    (hue.round() as u16) % 360
}

/// A texture for a pane that just became independent: a painted mode
/// not in `taken`.
fn distinct_texture(taken: &[TextureMode], random: f64) -> TextureMode {
    let pool: Vec<TextureMode> = TextureMode::all()
        .iter()
        .copied()
        .filter(|mode| *mode != TextureMode::None && !taken.contains(mode))
        .collect();
    let modes: &[TextureMode] = if pool.is_empty() {
        &TextureMode::ALL[1..]
    } else {
        &pool
    };
    let pick = (random.clamp(0.0, 1.0) * modes.len() as f64) as usize % modes.len();
    modes[pick]
}

impl PaneThemes {
    /// Themes for one workspace, inside the host's owner. `panes` is how
    /// many the workspace places.
    pub fn new(
        independent: RwSignal<bool>,
        independent_textures: RwSignal<bool>,
        shared_base: Signal<bool>,
        panes: Signal<usize>,
    ) -> Self {
        Self {
            independent,
            active: Signal::derive(move || independent.get() && panes.get() >= 2),
            independent_textures,
            textures_active: Signal::derive(move || independent_textures.get() && panes.get() >= 2),
            panes,
            version: RwSignal::new(0),
            overrides: StoredValue::new_local(Rc::new(RefCell::new(HashMap::new()))),
            shared_base,
        }
    }

    /// Whether Light / Dark / Dim is shared across the panes right now.
    fn shared_base(self) -> bool {
        self.shared_base.get_untracked()
    }

    /// Whether a look of its own is showing: the preference AND its split.
    fn in_effect(self) -> bool {
        self.active.get_untracked()
    }

    /// Whether the TEXTURE family is a pane's own: the texture preference
    /// or independent themes.
    fn textures_owned(self) -> bool {
        self.in_effect() || self.textures_active.get_untracked()
    }

    /// A pane's look as shown: a family the modes do not route is the
    /// window's.
    fn shown(self, mut look: Appearance, global: Appearance) -> Appearance {
        if self.in_effect() {
            if self.shared_base() {
                look.base = global.base;
            }
        } else {
            look.base = global.base;
            look.tint_hue = global.tint_hue;
            look.tint_strength = global.tint_strength;
        }
        if !self.textures_owned() {
            look.texture = global.texture;
            look.texture_opacity = global.texture_opacity;
            look.texture_scale = global.texture_scale;
        }
        // Grain has no per-pane half: a pane reports the window's values.
        look.noise = global.noise;
        look.noise_intensity = global.noise_intensity;
        look
    }

    /// `from` with a tint hue unlike every pane's showing now (and the
    /// window's, when tinted).
    fn distinct_colour(self, from: Appearance, global: Appearance) -> Appearance {
        let mut taken: Vec<u16> = self.overrides.with_value(|m| {
            m.borrow()
                .values()
                .filter(|look| look.has_tint())
                .map(|look| look.tint_hue)
                .collect()
        });
        if global.has_tint() {
            taken.push(global.tint_hue);
        }
        let mut look = from;
        look.tint_hue = distinct_hue(&taken, js_sys::Math::random());
        if !look.has_tint() {
            look.tint_strength = PANE_TINT_STRENGTH;
        }
        look.sanitize();
        look
    }

    /// `from` with a texture unlike every mode showing, when independent
    /// textures are on; otherwise `from`'s.
    pub fn distinct_look(self, from: Appearance, global: Appearance) -> Appearance {
        let mut look = if self.in_effect() {
            self.distinct_colour(from, global)
        } else {
            from
        };
        if self.textures_active.get_untracked() {
            let showing: Vec<TextureMode> = self
                .overrides
                .with_value(|m| m.borrow().values().map(|look| look.texture).collect());
            look.texture = distinct_texture(&showing, js_sys::Math::random());
        }
        look.sanitize();
        look
    }

    /// The STORED preference the menu's switch and the Settings row show.
    pub fn preferred(self) -> Signal<bool> {
        self.independent.into()
    }

    /// The texture family's STORED preference.
    pub fn preferred_textures(self) -> Signal<bool> {
        self.independent_textures.into()
    }

    /// Whether the texture mode is IN EFFECT. Tracked by the menu and the
    /// boundary push.
    pub fn textures_active(self) -> Signal<bool> {
        self.textures_active
    }

    /// Whether the mode is IN EFFECT: the preference while two or more
    /// panes are placed.
    pub fn active(self) -> Signal<bool> {
        self.active
    }

    /// How many panes the workspace places (the menu's split row keys off
    /// it).
    pub fn panes(self) -> Signal<usize> {
        self.panes
    }

    /// The change token the appearance boundary tracks alongside settings.
    pub fn version(self) -> RwSignal<u64> {
        self.version
    }

    fn bump(self) {
        self.version.update(|v| *v = v.wrapping_add(1));
    }

    /// The look this pane owns under a per-pane mode (`None` = the window
    /// theme).
    pub fn look_for(self, id: PaneId, global: Appearance) -> Option<Appearance> {
        (self.in_effect() || self.textures_active.get_untracked())
            .then(|| self.active_look(Some(id), global))
    }

    /// The look the window takes over as the last split collapses: the
    /// survivor's own.
    pub fn promote(self, survivor: PaneId, global: Appearance) -> Option<Appearance> {
        if !self.in_effect() && !self.textures_active.get_untracked() {
            return None;
        }
        let look = self.active_look(Some(survivor), global);
        (look != global).then_some(look)
    }

    /// The active pane's current look, what the menu's dials edit.
    fn active_look(self, active: Option<PaneId>, global: Appearance) -> Appearance {
        let look = active
            .and_then(|id| self.overrides.with_value(|m| m.borrow().get(&id).copied()))
            .unwrap_or(global);
        self.shown(look, global)
    }

    /// Seed (or overwrite) one pane's look. The caller decides what with.
    pub fn seed(self, id: PaneId, look: Appearance) {
        self.overrides.with_value(|m| {
            m.borrow_mut().insert(id, look);
        });
        self.bump();
    }

    /// A closed pane takes its look with it; the survivor's is the
    /// window's ([`Self::promote`]).
    pub fn forget(self, id: PaneId) {
        self.overrides.with_value(|m| {
            m.borrow_mut().remove(&id);
        });
        self.bump();
    }

    /// Turn the toggle on: the active pane keeps its look, the others get
    /// a colour.
    pub fn enable(
        self,
        placed: impl Iterator<Item = PaneId>,
        active: Option<PaneId>,
        global: Appearance,
    ) {
        let placed: Vec<PaneId> = placed.collect();
        self.independent.set(true);
        self.overrides.with_value(|m| {
            m.borrow_mut().retain(|id, _| placed.contains(id));
        });
        if placed.len() >= 2 {
            let first = active
                .filter(|id| placed.contains(id))
                .or_else(|| placed.first().copied());
            for id in placed {
                let stored = self.overrides.with_value(|m| m.borrow().get(&id).copied());
                let from = match stored {
                    Some(look) => Appearance {
                        texture: look.texture,
                        texture_opacity: look.texture_opacity,
                        texture_scale: look.texture_scale,
                        ..global
                    },
                    None => global,
                };
                let look = if Some(id) == first {
                    from
                } else {
                    self.distinct_colour(from, global)
                };
                self.overrides.with_value(|m| {
                    m.borrow_mut().insert(id, look);
                });
            }
        }
        self.bump();
    }

    /// Turn the texture toggle on: one pattern per pane, random and
    /// unlike the others.
    pub fn enable_textures(
        self,
        placed: impl Iterator<Item = PaneId>,
        active: Option<PaneId>,
        global: Appearance,
    ) {
        let placed: Vec<PaneId> = placed.collect();
        self.independent_textures.set(true);
        if placed.len() >= 2 {
            let first = active
                .filter(|id| placed.contains(id))
                .or_else(|| placed.first().copied());
            // The window's texture counts as taken, so no pane repeats it.
            let mut taken = vec![global.texture];
            for id in placed {
                let stored = self.overrides.with_value(|m| m.borrow().get(&id).copied());
                // The pane's own colour, the window's CURRENT texture.
                let mut look = match stored {
                    Some(look) => Appearance {
                        base: look.base,
                        tint_hue: look.tint_hue,
                        tint_strength: look.tint_strength,
                        ..global
                    },
                    None => global,
                };
                if Some(id) != first {
                    look.texture = distinct_texture(&taken, js_sys::Math::random());
                    taken.push(look.texture);
                }
                look.sanitize();
                self.overrides.with_value(|m| {
                    m.borrow_mut().insert(id, look);
                });
            }
        }
        self.bump();
    }

    /// Turn the texture toggle off: every pane shows the window's texture
    /// again.
    fn disable_textures(self, global: Appearance) {
        self.independent_textures.set(false);
        self.overrides.with_value(|m| {
            for look in m.borrow_mut().values_mut() {
                look.texture = global.texture;
                look.texture_opacity = global.texture_opacity;
                look.texture_scale = global.texture_scale;
            }
        });
        self.bump();
    }

    /// Turn the toggle off: the colour overrides go, unless independent
    /// textures still own their halves.
    fn disable(self, global: Appearance) {
        self.overrides.with_value(|m| {
            let mut m = m.borrow_mut();
            if self.textures_active.get_untracked() {
                for look in m.values_mut() {
                    look.base = global.base;
                    look.tint_hue = global.tint_hue;
                    look.tint_strength = global.tint_strength;
                }
            } else {
                m.clear();
            }
        });
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

    /// A scrub tick's live paint: the pane's look at the dragged values.
    pub fn preview(self, id: PaneId, look: Appearance) {
        self.overrides.with_value(|m| {
            m.borrow_mut().insert(id, look);
        });
        self.bump();
    }
}

/// Mark the document with the pane a slider drag edits (`None`: the
/// whole window).
fn scope_scrub(pane: Option<PaneId>) {
    const ATTR: &str = "data-appearance-scope";
    let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    else {
        return;
    };
    let _ = match pane {
        Some(id) => root.set_attribute(ATTR, &id.get().to_string()),
        None => root.remove_attribute(ATTR),
    };
}

/// Build the menu's theme handle for this workspace: routed arms follow the
/// routing rule above.
pub(crate) fn theme_handle(
    themes: PaneThemes,
    manager: super::manager::PaneManager,
    settings: RwSignal<Settings>,
) -> app_ui::appearance::ThemeHandle {
    use app_ui::appearance::{
        PaintTarget, ThemeScope, preview_appearance, preview_appearance_into,
    };

    // The family decides whether an edit can be a pane's own; grain never
    // is.
    let routes = move |scope: ThemeScope| {
        let owned = match scope {
            ThemeScope::Colour => themes.in_effect(),
            ThemeScope::Texture => themes.textures_owned(),
            ThemeScope::Global => false,
        };
        owned && manager.active().is_some()
    };

    let look = Signal::derive(move || {
        // The map's version is subscribed to here, as in the host paint
        // boundary.
        themes.version().with(|_| ());
        let global = settings.with(|s| s.appearance);
        // Either mode IN EFFECT routes; `active_look` answers per family.
        if themes.active().get() || themes.textures_active().get() {
            themes.active_look(manager.active(), global)
        } else {
            global
        }
    });

    let set_independent = Callback::new(move |on: bool| {
        // The toggle's rest state is a workspace setting; the colours it
        // carries are temporary.
        settings.update(|s| s.workspace.independent_themes = on);
        let global = settings.get_untracked().appearance;
        if on {
            themes.enable(manager.placed().into_iter(), manager.active(), global);
        } else {
            themes.disable(global);
        }
    });

    let commit = Callback::new(
        move |(scope, patch): (ThemeScope, app_ui::appearance::AppearancePatch)| {
            use app_ui::appearance::flush_appearance_commit;
            // A pending slider drag lands first, then the change applies.
            flush_appearance_commit();
            let routed = routes(scope);
            if routed {
                let id = manager.active().expect("checked: active pane exists");
                let global = settings.get_untracked().appearance;
                let before = themes.active_look(Some(id), global);
                let mut after = before;
                patch(&mut after);
                // Shared mode: a base switch from any pane is the window's, so
                // every pane follows.
                if themes.shared_base() && after.base != before.base {
                    settings.update(|s| {
                        s.appearance.base = after.base;
                        s.touch_appearance();
                    });
                }
                themes.edit(id, global, move |look| *look = after);
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
            let routed = routes(scope);
            // The engine scopes the scrub's raw-raster window by this mark.
            scope_scrub(routed.then(|| manager.active()).flatten());
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
                    // Tick paints wrote these values; nothing to land.
                    Box::new(|_p| {}),
                );
            } else {
                preview_appearance(settings, patch);
            }
        },
    );

    let set_independent_texture = Callback::new(move |on: bool| {
        // Same persistence rule as the colour toggle: the preference
        // persists, the per-pane textures do not.
        settings.update(|s| s.workspace.independent_textures = on);
        let global = settings.get_untracked().appearance;
        if on {
            themes.enable_textures(manager.placed().into_iter(), manager.active(), global);
        } else {
            themes.disable_textures(global);
        }
    });

    app_ui::appearance::ThemeHandle {
        look,
        independent: themes.preferred(),
        set_independent,
        independent_texture: themes.preferred_textures(),
        set_independent_texture,
        commit,
        scrub,
        panes: themes.panes(),
    }
}

#[cfg(test)]
mod tests {
    use super::{distinct_hue, distinct_texture};

    fn gap(a: u16, b: u16) -> u16 {
        let d = a.abs_diff(b) % 360;
        d.min(360 - d)
    }

    #[test]
    fn any_hue_when_nothing_is_taken() {
        assert_eq!(distinct_hue(&[], 0.0), 0);
        assert_eq!(distinct_hue(&[], 0.5), 180);
        assert!(distinct_hue(&[], 1.0) < 360);
    }

    #[test]
    fn a_new_hue_lands_well_away_from_every_taken_one() {
        for r in [0.0, 0.3, 0.7, 1.0] {
            let h = distinct_hue(&[10], r);
            assert!(gap(h, 10) >= 90, "{h} too near 10");
            let taken = [0, 120, 240];
            let h = distinct_hue(&taken, r);
            assert!(
                taken.iter().all(|t| gap(h, *t) >= 30),
                "{h} too near {taken:?}"
            );
        }
    }

    #[test]
    fn the_widest_gap_wins_across_zero() {
        // Taken 100..=200: the free arc is 200 -> 460 (= 100), centre 330.
        assert_eq!(distinct_hue(&[100, 150, 200], 0.5), 330);
    }

    #[test]
    fn a_new_texture_skips_the_modes_already_showing() {
        use reader_core::appearance::TextureMode;
        let taken = [TextureMode::Paper, TextureMode::Grid];
        for r in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let mode = distinct_texture(&taken, r);
            assert!(!taken.contains(&mode), "{mode:?} was already taken");
            assert_ne!(mode, TextureMode::None, "None paints nothing");
        }
        // Every textured mode taken: still a textured mode, at random.
        let full: Vec<TextureMode> = TextureMode::all().iter().copied().skip(1).collect();
        assert_ne!(distinct_texture(&full, 0.5), TextureMode::None);
    }

    #[test]
    fn successive_panes_all_differ() {
        let mut taken = vec![];
        for r in [0.1, 0.9, 0.4, 0.6, 0.2] {
            let h = distinct_hue(&taken, r);
            assert!(
                taken.iter().all(|t: &u16| gap(h, *t) >= 15),
                "{h} vs {taken:?}"
            );
            taken.push(h);
        }
    }
}
