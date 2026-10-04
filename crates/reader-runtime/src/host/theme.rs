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
//!
//! Colours: turning the toggle on keeps the active pane's look and gives
//! every other pane a tint hue of its own, and a pane born while it is on
//! gets one too — picked at random inside the widest gap between the hues
//! already showing, so no two panes come out alike.
//!
//! Shared mode (`workspace.shared_base_mode`, on by default): only the
//! colour is per pane. Light / Dark / Dim stays the window's — every pane
//! shows the global base, and switching it from any pane switches all.

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

impl PaneThemes {
    /// Themes for one workspace. Call inside the host's owner (the signals
    /// live in the host's arena).
    pub fn new(independent: RwSignal<bool>, shared_base: Signal<bool>) -> Self {
        Self {
            independent,
            version: RwSignal::new(0),
            overrides: StoredValue::new_local(Rc::new(RefCell::new(HashMap::new()))),
            shared_base,
        }
    }

    /// Whether Light / Dark / Dim is shared across the panes right now.
    fn shared_base(self) -> bool {
        self.shared_base.get_untracked()
    }

    /// A pane's stored look as it shows: in shared mode the base is the
    /// window's, whatever the pane stored.
    fn shown(self, mut look: Appearance, global: Appearance) -> Appearance {
        if self.shared_base() {
            look.base = global.base;
        }
        look
    }

    /// A look of its own for a new pane: `from`'s, with a tint hue unlike
    /// every pane's showing now (and the window's, when it is tinted).
    pub fn distinct_look(self, from: Appearance, global: Appearance) -> Appearance {
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
        let look = self
            .overrides
            .with_value(|m| m.borrow().get(&id).copied())
            .unwrap_or(global);
        Some(self.shown(look, global))
    }

    /// The active pane's current look — what the menu's dials edit and show
    /// while independent themes are on.
    fn active_look(self, active: Option<PaneId>, global: Appearance) -> Appearance {
        let look = active
            .and_then(|id| self.overrides.with_value(|m| m.borrow().get(&id).copied()))
            .unwrap_or(global);
        self.shown(look, global)
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

    /// Turn the toggle on: the `active` pane keeps the global look and
    /// every other placed pane gets a colour of its own, each unlike the
    /// rest. The global theme itself is left untouched (remembered).
    pub fn enable(
        self,
        placed: impl Iterator<Item = PaneId>,
        active: Option<PaneId>,
        global: Appearance,
    ) {
        self.overrides.with_value(|m| m.borrow_mut().clear());
        let placed: Vec<PaneId> = placed.collect();
        let first = active
            .filter(|id| placed.contains(id))
            .or_else(|| placed.first().copied());
        for id in placed {
            let look = if Some(id) == first {
                global
            } else {
                self.distinct_look(global, global)
            };
            self.overrides.with_value(|m| {
                m.borrow_mut().insert(id, look);
            });
        }
        self.independent.set(true);
        self.bump();
    }

    /// Turn the toggle off: every override goes with it and the panes
    /// inherit the window theme again. What the panes painted as their own
    /// is removed by the pane paint on the next boundary push (`look: None`).
    fn disable(self) {
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

    let look = Signal::derive(move || {
        // The per-pane map is behind StoredValue/RefCell so it has one owner,
        // not one signal per pane. Subscribe to its version here as well as in
        // the host paint boundary: the menu's selected base, hue/strength
        // dials and active look must follow a focused pane's edit immediately.
        themes.version().with(|_| ());
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
            themes.enable(manager.placed().into_iter(), manager.active(), global);
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
            let routed = scope == ThemeScope::Routed
                && themes.independent.get_untracked()
                && manager.active().is_some();
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

    let panes = Signal::derive(move || manager.placed().len());

    app_ui::appearance::ThemeHandle {
        look,
        independent: themes.independent(),
        set_independent,
        commit,
        scrub,
        panes,
    }
}

#[cfg(test)]
mod tests {
    use super::distinct_hue;

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
