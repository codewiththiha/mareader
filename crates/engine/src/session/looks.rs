//! Per-pane appearance looks, routing, and independent text inside [`Session`].

use mareader_appearance::{Appearance, PaneLooks, Routing};
use mareader_settings::{Settings, TextSettings};
use mareader_workspace::PaneId;

use crate::route::Route;

use super::Session;

/// A visual snapshot held back during a 1-frame cross-fade capture.
#[derive(Clone)]
pub struct AppearanceSnapshot {
    settings: Settings,
    looks: PaneLooks<PaneId>,
    routed: Routing,
}

impl AppearanceSnapshot {
    /// The settings captured when the snapshot was taken.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Whether the window or any pane jumped to a different look since `self`.
    pub fn leapt(&self, current: &Session) -> bool {
        let (old, new) = (&self.settings, current.settings());
        let was = routing(old, current.tree().len() > 1);
        let jumped = |id: PaneId| {
            let from = self.looks.shown(id, &old.appearance, was);
            from.leaps_to(&current.look(id))
        };
        old.paper.blend != new.paper.blend
            || old.appearance.leaps_to(&new.appearance)
            || current.tree().panes().into_iter().any(jumped)
    }
}

impl Session {
    /// Which appearance families split panes own right now.
    pub fn routing(&self) -> Routing {
        routing(&self.settings, self.tree.len() > 1)
    }

    /// `pane`'s look as drawn: the window's, or its own while split.
    pub fn look(&self, pane: PaneId) -> Appearance {
        self.looks
            .shown(pane, &self.settings.appearance, self.routing())
    }

    /// `pane`'s text settings: the window's, or its own while independent.
    pub fn text(&self, pane: PaneId) -> &TextSettings {
        self.texts.of(pane, &self.settings.text)
    }

    /// Each pane with text settings of its own, mirroring the focused pane.
    pub fn own_texts(&self) -> Vec<(PaneId, &TextSettings)> {
        self.texts.own(&self.settings.text)
    }

    /// Captures appearance state for a front-end cross-fade transition.
    pub fn appearance_snapshot(&self) -> AppearanceSnapshot {
        AppearanceSnapshot {
            settings: self.settings.clone(),
            looks: self.looks.clone(),
            routed: self.routed,
        }
    }

    /// Temporarily swaps appearance state for a front-end cross-fade frame.
    pub fn restore_appearance(&mut self, held: AppearanceSnapshot) {
        let mut settings = held.settings;
        settings.sanitize();
        self.settings = settings;
        self.looks = held.looks;
        self.routed = held.routed;
    }

    /// Fits per-pane text settings to the current panes and focused pane.
    pub fn reconcile_texts(&mut self) {
        let on = self.settings.workspace.independent_text;
        let placed = self.tree.panes();
        let mut window = self.settings.text.clone();
        self.texts
            .reconcile(on, (&placed, self.active), &mut window);
        if window != self.settings.text {
            self.settings.text = window;
            self.save.touch();
        }
    }

    /// Fits per-pane looks to this frame's routing and open panes.
    pub fn reconcile_looks(&mut self, now: f64) {
        let routing_now = self.routing();
        let placed = self.tree.panes();
        let mut state = self.random ^ now.to_bits();
        let mut random = || {
            state = splitmix(state);
            (state >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut global = self.settings.appearance;
        let changes = (self.routed, routing_now);
        self.looks
            .reconcile(changes, (&placed, self.active), &mut global, &mut random);
        if global != self.settings.appearance {
            self.settings.appearance = global;
            self.save.touch();
        }
        self.random = state;
        self.routed = routing_now;
    }

    /// Applies an appearance control edit to the focused pane or window.
    pub(super) fn set_appearance(&mut self, look: Appearance) {
        let routing = self.routing();
        let mut global = self.settings.appearance;
        if self.route == Route::Reader {
            self.looks.edit(self.active, &mut global, routing, look);
        } else {
            self.looks.edit_window(&mut global, routing, look);
        }
        if global != self.settings.appearance {
            self.settings.appearance = global;
            self.save.touch();
        }
    }
}

/// Which families split panes own under `settings`.
fn routing(settings: &Settings, split: bool) -> Routing {
    let w = &settings.workspace;
    Routing {
        themes: w.independent_themes,
        textures: w.independent_textures,
        shared_base: w.shared_base_mode,
        split,
    }
}

/// SplitMix64: a well-spread next state from any state.
fn splitmix(state: u64) -> u64 {
    let mut z = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}
