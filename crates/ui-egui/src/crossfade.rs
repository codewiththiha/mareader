//! Theme and paper transitions: live token and shader blends with no readback.

use std::collections::BTreeMap;

use egui::{Color32, Context};
use mareader_appearance::{FilmGrain, Rgb};
use mareader_workspace::PaneId;

use crate::theme::Tokens;

/// How long a look transition takes, in seconds.
pub const FADE: f32 = 0.2;

/// Visual state captured right before a discrete appearance or paper change.
#[derive(Clone, Debug)]
pub struct LookSnap {
    pub tokens: Tokens,
    pub grain: Option<FilmGrain>,
    pub panes: BTreeMap<PaneId, PaneSnap>,
}

/// One pane's visual look at the instant a transition starts.
#[derive(Clone, Copy, Debug)]
pub struct PaneSnap {
    pub tokens: Tokens,
    pub fill: Color32,
    pub paper: Option<Rgb>,
}

/// Active transition state for one pane, blending `tokens`/`paper` toward now.
#[derive(Clone, Copy, Debug)]
pub struct PaneFade {
    pub tokens: Tokens,
    pub paper: Option<Rgb>,
    pub progress: f32,
}

/// Live cross-fade between the prior visual snapshot and the current look.
#[derive(Default)]
pub struct LookFade {
    active: Option<Active>,
}

struct Active {
    from: LookSnap,
    start: f64,
    progress: f32,
}

impl LookFade {
    /// Starts a live fade from `from` at `now`.
    pub fn start(&mut self, now: f64, from: LookSnap) {
        self.active = Some(Active {
            from,
            start: now,
            progress: 0.0,
        });
    }

    /// Cancels any active transition so an instant change lands cleanly.
    pub fn clear(&mut self) {
        self.active = None;
    }

    /// Advances the active fade for this frame; returns true while fading.
    pub fn advance(&mut self, ctx: &Context) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        let t = ((ctx.input(|i| i.time) - active.start) as f32 / FADE).clamp(0.0, 1.0);
        if t >= 1.0 {
            self.active = None;
            return false;
        }
        // Ease out: most of the change lands early, settling softly.
        active.progress = 1.0 - (1.0 - t) * (1.0 - t);
        ctx.request_repaint();
        true
    }

    pub fn is_fading(&self) -> bool {
        self.active.is_some()
    }

    /// Current window tokens blended from the prior snapshot toward `target`.
    pub fn tokens(&self, target: &Tokens) -> Tokens {
        match &self.active {
            Some(active) => active.from.tokens.toward(target, active.progress),
            None => *target,
        }
    }

    /// Current pane tokens, fill, and GPU fade payload toward `target`.
    pub fn pane(
        &self,
        id: PaneId,
        target_tokens: &Tokens,
        target_fill: Color32,
    ) -> (Tokens, Color32, Option<PaneFade>) {
        let Some(active) = &self.active else {
            return (*target_tokens, target_fill, None);
        };
        let fallback = PaneSnap {
            tokens: active.from.tokens,
            fill: active.from.tokens.paper,
            paper: None,
        };
        let from = active.from.panes.get(&id).copied().unwrap_or(fallback);
        let k = active.progress;
        let tokens = from.tokens.toward(target_tokens, k);
        let fill = from.fill.lerp_to_gamma(target_fill, k);
        let fade = PaneFade {
            tokens: from.tokens,
            paper: from.paper,
            progress: k,
        };
        (tokens, fill, Some(fade))
    }

    /// Prior grain fading out and `target` grain fading in.
    pub fn grain(&self, target: Option<FilmGrain>) -> (Option<FilmGrain>, Option<FilmGrain>) {
        let Some(active) = &self.active else {
            return (None, target);
        };
        if active.from.grain == target {
            return (None, target);
        }
        let k = active.progress;
        let scale = |grain: Option<FilmGrain>, factor: f32| {
            grain
                .map(|mut g| {
                    g.opacity *= factor;
                    g
                })
                .filter(|g| g.opacity > 0.001)
        };
        (scale(active.from.grain, 1.0 - k), scale(target, k))
    }
}

#[cfg(test)]
mod tests {
    use mareader_appearance::{Appearance, BaseMode, NoiseMode};

    use super::*;

    fn snap(appearance: &Appearance) -> LookSnap {
        let tokens = Tokens::new(appearance);
        LookSnap {
            tokens,
            grain: FilmGrain::of(appearance),
            panes: BTreeMap::new(),
        }
    }

    fn step_at(ctx: &Context, time: f64, mut f: impl FnMut()) {
        let mut out = ctx.run_ui(
            egui::RawInput {
                time: Some(time),
                ..Default::default()
            },
            |_| f(),
        );
        out.textures_delta.clear();
    }

    #[test]
    fn fade_blends_tokens_and_finishes_cleanly() {
        let ctx = Context::default();
        let light = Appearance {
            base: BaseMode::Light,
            ..Appearance::default()
        };
        let dark = Appearance {
            base: BaseMode::Dark,
            ..Appearance::default()
        };
        let from = Tokens::new(&light);
        let to = Tokens::new(&dark);
        let mut fade = LookFade::default();
        fade.start(1.0, snap(&light));
        assert!(fade.is_fading());
        assert_eq!(fade.tokens(&to).paper, from.paper);

        step_at(&ctx, 1.0 + f64::from(FADE) * 0.5, || {
            assert!(fade.advance(&ctx));
        });
        let mid = fade.tokens(&to);
        assert!(mid.paper.r() < from.paper.r() && mid.paper.r() > to.paper.r());

        step_at(&ctx, 1.0 + f64::from(FADE) + 0.05, || {
            assert!(!fade.advance(&ctx));
        });
        assert!(!fade.is_fading());
        assert_eq!(fade.tokens(&to).paper, to.paper);
    }

    #[test]
    fn grain_crossfades_opacity_and_skips_unchanged() {
        let ctx = Context::default();
        let plain = Appearance {
            base: BaseMode::Light,
            ..Appearance::default()
        };
        let grainy = Appearance {
            base: BaseMode::Light,
            noise: NoiseMode::Static,
            noise_intensity: 40,
            ..Appearance::default()
        };
        let mut fade = LookFade::default();
        fade.start(0.0, snap(&plain));
        step_at(&ctx, f64::from(FADE) * 0.5, || {
            assert!(fade.advance(&ctx));
        });
        let (old, new) = fade.grain(FilmGrain::of(&grainy));
        assert!(old.is_none());
        let new = new.unwrap();
        assert!(new.opacity > 0.25 && new.opacity < 0.35);

        let mut steady = LookFade::default();
        steady.start(0.0, snap(&grainy));
        let (old, new) = steady.grain(FilmGrain::of(&grainy));
        assert!(old.is_none());
        assert_eq!(new, FilmGrain::of(&grainy));
    }
}
