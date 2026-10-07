//! The workspace's independent theme state: one look per pane, toggled
//! from the appearance menu while a split workspace is on screen.
//!
//! Routing rule (the walkthrough's), per FAMILY: a colour edit — base mode
//! or tint — goes to the ACTIVE pane's own look while independent themes are
//! IN EFFECT; a texture edit — the mode or either dial — goes there while
//! independent themes OR independent textures are in effect. Anything else,
//! including both while no per-pane mode is on, is an ordinary Settings edit,
//! and the film grain dial is the one dial that stays global whatever the
//! toggles say. The shared chrome (title bar, sidebar, the backdrop outside
//! panes) keeps the remembered global theme throughout; only a pane's own box
//! shows its own look. Two preferences, one map: an `Appearance` per pane is
//! whole, and each family of it is read from the pane or the window by the
//! rules below, which is why one `version` bump and one push carry either.
//!
//! The mode needs the split it serves, so it is in effect only while two or
//! more panes are placed: with ONE pane left, per-pane theming has nothing to
//! distinguish, and a pane coloured unlike the chrome around it is exactly
//! the mismatch the single-pane state must not show. The stored preference
//! (`settings.workspace.independent_themes`) is what the menu's switch and
//! the Settings row carry, and it survives the stand-down: the next split
//! brings the mode back by itself, unless the reader switched it off.
//!
//! Lifetime rules, from the close/fallback walkthrough (4 panes
//! blue/red/yellow/green): the working colour is the ACTIVE pane's colour —
//! a non-active close never moves focus, an active close hands it to the
//! tree's successor — so no separate "last activated" tracker exists. An
//! override survives in-place document opens (it is keyed by the PANE id,
//! which a replace keeps) and dies with its pane. On split exit the last
//! pane's colour is not lost: it is promoted to the window theme as the mode
//! stands down ([`PaneThemes::promote`]), so the surviving pane and the
//! chrome agree by construction, and the re-split that follows seeds its new
//! pane beside it. Turning the toggle off by hand clears every override and
//! the panes inherit again — with one exception: the texture halves of a split
//! that has independent textures on belong to that preference, and a colour
//! switch does not empty what it does not route ([`PaneThemes::disable`]).
//!
//! Colours: turning the toggle on keeps the active pane's look and gives
//! every other pane a tint hue of its own, and a pane born while it is on
//! gets one too — picked at random inside the widest gap between the hues
//! already showing, so no two panes come out alike. Textures: the same shape
//! of answer — the focused pane keeps what it shows, every other pane gets a
//! mode of its own drawn from the modes not already showing, and closing the
//! mode folds the window's texture back into every pane so the workspace is
//! one texture again. The last pane's texture is promoted to the window on the
//! same close that promotes its colour, and it persists in settings the same
//! way, so the next launch opens with the texture the reader left.
//!
//! Shared mode (`workspace.shared_base_mode`, on by default): only the
//! colour is per pane. Light / Dark / Dim stays the window's — every pane
//! shows the global base, and switching it from any pane switches all.
//!
//! A preset is a colour-family edit, so it follows the colour rule: routed it
//! is one pane's whole look, patterns included; global it is the window's
//! look for every family a pane does not own, which leaves each pane's own
//! pattern where it was — the picker's chips are how a pattern moves.
//!
//! The texture family (`workspace.independent_textures`) is the same
//! mechanism over one family instead of the whole look, for the reader who
//! wants a Lined PDF beside a plain page: it owns the texture mode and its
//! two dials, and it composes with the colour mode rather than replacing it.
//! Each pane's shown look is therefore assembled per family — whatever a
//! pane does not own it takes from the window, so no stored half can outlive
//! the preference that routed it. Turning the texture toggle off hands every
//! pane the window's texture again and leaves the colour halves the map holds
//! untouched. The colour switch is held to the same limit: switching it on
//! gives each pane a hue of its own without reshuffling the patterns it shows,
//! and switching it off folds the colour halves onto the window's where a
//! texture mode still owns the map. A family the mode no longer owns is never
//! shown, only stored — and no mode ever clears the other's.

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
    /// The STORED preference. Persisted with the workspace settings by the
    /// shell; the host only reads and publishes it. Whether it is in effect
    /// is [`Self::active`]'s answer.
    independent: RwSignal<bool>,
    /// The texture family's own preference, with the same split gate: a page's
    /// pattern is a decision about the paper, not about the look, so it can be
    /// per pane while the colour is shared.
    independent_textures: RwSignal<bool>,
    /// The preference AND the split it serves: derived once here, so the
    /// pane-count read has one home.
    active: Signal<bool>,
    textures_active: Signal<bool>,
    /// The pane count the stand-down follows (the workspace's placement).
    panes: Signal<usize>,
    /// Bumped on every map or toggle change: the host's appearance boundary
    /// tracks it and re-pushes each pane's effective look.
    version: RwSignal<u64>,
    overrides: StoredValue<Rc<RefCell<HashMap<PaneId, Appearance>>>, LocalStorage>,
    /// Whether Light / Dark / Dim stays shared across panes (the setting).
    shared_base: Signal<bool>,
}

/// The tint strength a pane's own colour gets when the look it starts from
/// has none: clearly its own colour, still quiet enough to read on.
const PANE_TINT_STRENGTH: u8 = 35;

/// A hue for a new pane: somewhere in the middle of the widest gap between
/// the hues `taken` (degrees), jittered by `random` in `0..1` so a workspace
/// does not always get the same colours. With nothing taken, any hue.
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

/// A texture for a pane that has just become independent: one at random from
/// the modes that paint something, and unlike every `taken` mode while any
/// remain. `None` is out of the pool — an "independent texture" that shows no
/// texture is not independent, it is off — and with five modes and at most
/// four panes the pool cannot run dry; the fallback is there to keep the
/// choice total if that ever changes.
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
    /// Themes for one workspace. Call inside the host's owner (the signals
    /// live in the host's arena). `panes` is how many the workspace places:
    /// one pane is no split, and the mode stands down there.
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

    /// Whether a look of its own is showing anywhere: the stored preference
    /// AND the split it serves. The untracked gate every routing read uses.
    fn in_effect(self) -> bool {
        self.active.get_untracked()
    }

    /// Whether the TEXTURE family is a pane's own: its preference (with the
    /// split it serves), or independent themes, which have always owned the
    /// whole look — texture included. One answer for the picker's routing and
    /// for the composition below, so the two can never disagree.
    fn textures_owned(self) -> bool {
        self.in_effect() || self.textures_active.get_untracked()
    }

    /// A pane's stored look as it shows, assembled family by family: a family
    /// the modes do not route to the pane is the window's, whatever the map
    /// still holds from an earlier mode. `shared_base_mode` is the same rule
    /// for one field of the colour family.
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
        // Grain has no per-pane half at all: the noise layer is the window's,
        // so a pane's look reports it as the window's rather than as whatever
        // the map happened to be holding when the preset was clicked.
        look.noise = global.noise;
        look.noise_intensity = global.noise_intensity;
        look
    }

    /// `from` with a tint hue unlike every pane's showing now (and the
    /// window's, when it is tinted). `shared_base_mode` plays no part here: it
    /// narrows what a pane's look SHOWS ([`Self::shown`]), not what the pane
    /// owns, so an override written under a shared base is still the pane's own
    /// colour for the day the sharing stops.
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

    /// `from` with a TEXTURE unlike every mode showing now, when the reader has
    /// the texture mode on. With the texture mode off the pane keeps `from`'s
    /// texture: independent themes inherit the look that is on screen, which is
    /// what a split beside a textured PDF is for.
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

    /// The STORED preference — what the menu's switch and the Settings row
    /// show and flip, whether or not a split is on screen to carry it.
    pub fn preferred(self) -> Signal<bool> {
        self.independent.into()
    }

    /// The texture family's STORED preference.
    pub fn preferred_textures(self) -> Signal<bool> {
        self.independent_textures.into()
    }

    /// Whether the texture mode is IN EFFECT (its preference and the split it
    /// serves). Tracked: the menu's row and the boundary push both follow a
    /// pane count change through it.
    pub fn textures_active(self) -> Signal<bool> {
        self.textures_active
    }

    /// Whether the mode is IN EFFECT: the preference, while two or more
    /// panes are placed. Tracked: the class the blend rules key off and the
    /// workspace look follow a pane count change. Everything that shows or
    /// routes a per-pane look reads this, never the preference.
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

    /// The look this pane owns while a per-pane mode is in effect (`None` =
    /// inherit the window theme, which is also what a family the modes do not
    /// route shows). A pane missing from the map answers the defensive
    /// `global` — seeding keeps that arm unreachable in practice.
    pub fn look_for(self, id: PaneId, global: Appearance) -> Option<Appearance> {
        if !self.in_effect() && !self.textures_active.get_untracked() {
            return None;
        }
        let look = self
            .overrides
            .with_value(|m| m.borrow().get(&id).copied())
            .unwrap_or(global);
        Some(self.shown(look, global))
    }

    /// The look the window takes over as the last split collapses: the
    /// surviving pane's own, so the single pane left and the chrome that
    /// surrounds it agree by construction. `None` when there is nothing to
    /// hand over: the mode is not in effect, or that pane already shows
    /// exactly the window theme.
    ///
    /// Call BEFORE the close lands the count at one (the mode is still live
    /// then); the caller writes the answer into Settings.
    pub fn promote(self, survivor: PaneId, global: Appearance) -> Option<Appearance> {
        if !self.in_effect() && !self.textures_active.get_untracked() {
            return None;
        }
        let look = self.active_look(Some(survivor), global);
        (look != global).then_some(look)
    }

    /// The active pane's current look — what the menu's dials edit and show
    /// while independent themes are in effect.
    fn active_look(self, active: Option<PaneId>, global: Appearance) -> Appearance {
        let look = active
            .and_then(|id| self.overrides.with_value(|m| m.borrow().get(&id).copied()))
            .unwrap_or(global);
        self.shown(look, global)
    }

    /// Seed (or overwrite) one pane's look. The caller decides what a pane is
    /// seeded with: a pane born into a live split takes the ACTIVE pane's
    /// shown look in a family of its own, and switching a per-pane mode on
    /// seeds every placed pane from the window's current values for the family
    /// that switch owns and the pane's stored halves for the family it does
    /// not — never from [`Self::active_look`], because a half folded by the
    /// other mode's teardown is not a choice the pane made since.
    pub fn seed(self, id: PaneId, look: Appearance) {
        self.overrides.with_value(|m| {
            m.borrow_mut().insert(id, look);
        });
        self.bump();
    }

    /// A closed pane takes its look with it. The surviving pane's colour is
    /// never lost: with one pane left it is the window's ([`Self::promote`]).
    /// Every path that ends a pane runs this, including a placement the tree
    /// refused: that pane was seeded before the split was checked, and ids are
    /// never reused, so a look left behind is a look nothing will ever show.
    pub fn forget(self, id: PaneId) {
        self.overrides.with_value(|m| {
            m.borrow_mut().remove(&id);
        });
        self.bump();
    }

    /// Turn the toggle on: the `active` pane keeps the look it shows and every
    /// other placed pane gets a colour of its own, each unlike the rest. The
    /// global theme itself is left untouched (remembered).
    ///
    /// A lone pane has no split to show a colour in: the preference arms here
    /// and the mode shows itself on the next split, which seeds the pane born
    /// beside it. Seeding a lone pane now would only hand it a snapshot of a
    /// window theme the reader may still edit before that split arrives.
    ///
    /// Each pane is seeded from the window's colour plus its OWN texture
    /// family: the colour family is this switch's to decide, and a reader who
    /// already had a texture per pane keeps those patterns (the family the
    /// texture preference owns is not shuffled by a colour switch). The colour
    /// half comes from the live global look rather than from what the pane
    /// shows, because `disable` folded those halves when this mode went off
    /// and the window may have moved since — a stored colour must not come
    /// back on its own. The map is trimmed to the placed panes rather than
    /// emptied, which is the same guard it always was — a closed pane forgets
    /// its own look — written so it cannot reach past this family.
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

    /// Turn the texture toggle on: the pane in front keeps the window's
    /// texture and every other placed pane gets one of its own, chosen at
    /// random and unlike the others, so the split opens on a texture per pane
    /// instead of on the same pattern five times. The window's own texture is
    /// left as it was (remembered), and the colour halves of each pane's look
    /// ride along untouched — this preference owns one family, and it is
    /// seeded from the live window rather than from what the map still holds
    /// for the family it is about to decide.
    ///
    /// The lone-pane rule is the colour toggle's: the preference arms, the
    /// mode shows itself on the next split.
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
            // The window's own texture counts as taken: no second pane is
            // handed the pattern the reader just turned independence off to
            // escape.
            let mut taken = vec![global.texture];
            for id in placed {
                let stored = self.overrides.with_value(|m| m.borrow().get(&id).copied());
                // The pane's own colour, the window's CURRENT texture: this
                // switch decides the texture family, and a family it decides
                // is seeded from where it lives now rather than from the fold
                // the last mode left in the map — a pattern the reader has
                // since changed on the window must not come back as a pane's
                // "own" choice.
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

    /// Turn the texture toggle off by hand: every pane shows the window's
    /// texture again, and the per-pane picks go with the mode rather than
    /// waiting under it hidden — the colour halves of the same looks stay,
    /// because this preference never owned them. A stand-down (one pane left)
    /// is NOT this: the map survives it, and the survivor's texture is
    /// promoted to the window first ([`Self::promote`]).
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

    /// Turn the toggle off by hand: the colour overrides go with it and the
    /// panes inherit the window theme again. What the panes painted as their
    /// own is removed by the pane paint on the next boundary push
    /// (`look: None`). A stand-down is NOT this: it keeps the map, so the
    /// next split brings the colours back.
    ///
    /// The one case the map survives is a split that still has its own
    /// texture: `independent_textures` owns those halves, so emptying the map
    /// here would erase patterns the colour switch never claimed. Folding the
    /// colour halves onto the window instead leaves the map holding exactly
    /// what the reader has left selected — and nothing stale to resurface if
    /// this mode comes back on later.
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

/// Mark the document with the pane a slider drag is editing (`None`: the
/// whole window). Read by the PDF engine when the drag's scrub begins.
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

    // Which family an edit belongs to decides whether it can be a pane's own:
    // a colour edit while independent themes are in effect, a texture edit
    // while either mode is — independent themes have always owned the whole
    // look, texture included, and the texture preference adds the family on its
    // own. Film grain answers `false`: it is the window's dial.
    // A `move` closure: both callbacks below capture it, and the two handles
    // it reads are Copy, so the rule itself is Copy.
    let routes = move |scope: ThemeScope| {
        let owned = match scope {
            ThemeScope::Colour => themes.in_effect(),
            ThemeScope::Texture => themes.textures_owned(),
            ThemeScope::Global => false,
        };
        owned && manager.active().is_some()
    };

    let look = Signal::derive(move || {
        // The per-pane map is behind StoredValue/RefCell so it has one owner,
        // not one signal per pane. Subscribe to its version here as well as in
        // the host paint boundary: the menu's selected base, hue/strength
        // dials and active look must follow a focused pane's edit immediately.
        themes.version().with(|_| ());
        let global = settings.with(|s| s.appearance);
        // The route follows either mode IN EFFECT: a look of its own is a
        // look of its own, whichever preference put it there, and
        // `active_look` answers each family by its own rule (the colour half
        // the window's while only textures are independent). At one pane both
        // modes stand down and the dials show and edit the window theme,
        // exactly what that pane shows.
        if themes.active().get() || themes.textures_active().get() {
            themes.active_look(manager.active(), global)
        } else {
            global
        }
    });

    let set_independent = Callback::new(move |on: bool| {
        // The toggle's rest state is a workspace setting; the per-pane
        // colours it carries are temporary and never persisted.
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
            // A pending slider drag lands first (the shared scheduler's
            // contract), then the structural change applies on top.
            flush_appearance_commit();
            let routed = routes(scope);
            if routed {
                let id = manager.active().expect("checked: active pane exists");
                let global = settings.get_untracked().appearance;
                let before = themes.active_look(Some(id), global);
                let mut after = before;
                patch(&mut after);
                // Shared mode: a Light / Dark / Dim switch from any pane is
                // the window's, so every pane follows it; the rest of the
                // edit stays this pane's own.
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
            // The engine scopes the scrub's raw-raster window by this mark:
            // a drag on one pane's look leaves every other pane's pages as
            // they are (public/pdfEngine.ts `scrubScope`).
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
                    // The tick paints already wrote the map at exactly these
                    // values; a settled commit has nothing left to land.
                    Box::new(|_p| {}),
                );
            } else {
                preview_appearance(settings, patch);
            }
        },
    );

    let set_independent_texture = Callback::new(move |on: bool| {
        // Same persistence rule as the colour toggle: the preference is the
        // workspace's, the per-pane textures it carries are temporary, and the
        // window's own texture is what every pane falls back to when the mode
        // goes off.
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
        // Every textured mode taken (five) and a sixth asked for: still a
        // textured mode, one of the pool chosen at random.
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
