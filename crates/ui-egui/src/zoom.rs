//! Zoom: the preset ladder, the fit modes, and the zoom a pane reports.

use egui::Vec2;
use mareader_session::PageSize;

pub const MIN: f32 = 0.25;
pub const MAX: f32 = 5.0;

/// The ladder `+` and `-` climb; 25 to 500 %.
pub const PRESETS: [f32; 15] = [
    0.25, 0.33, 0.5, 0.67, 0.75, 0.9, 1.0, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0, 4.0, 5.0,
];

/// Zooms closer than this are the same zoom.
const SAME: f32 = 0.005;

use mareader_engine::pane::{Fit, Step, ZoomCommand};

/// A pane's zoom, as the menu shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomState {
    pub zoom: f32,
    pub fit: Option<Fit>,
}

impl ZoomState {
    /// `125%`; the target, so a running tween doesn't flicker digits.
    pub fn percent(&self) -> String {
        format!("{:.0}%", self.zoom * 100.0)
    }

    pub fn can_step(&self, step: Step) -> bool {
        next(self.zoom, step).is_some()
    }
}

/// Garbage collapses to the smallest zoom, never into geometry.
pub fn clamp(zoom: f32) -> f32 {
    if zoom.is_finite() {
        zoom.clamp(MIN, MAX)
    } else {
        MIN
    }
}

/// The next preset beyond `zoom`; none past either end.
pub fn next(zoom: f32, step: Step) -> Option<f32> {
    match step {
        Step::In => PRESETS.iter().copied().find(|p| *p > zoom + SAME),
        Step::Out => PRESETS.iter().rev().copied().find(|p| *p < zoom - SAME),
    }
}

/// The zoom that fits `page` into `room`, edge to edge.
pub fn fit(fit: Fit, room: Vec2, page: PageSize) -> f32 {
    clamp(span(fit, room, page))
}

/// A fit's zoom before the clamp, so ratios between two stay true.
fn span(fit: Fit, room: Vec2, page: PageSize) -> f32 {
    let width = room.x.max(1.0) / page.width.max(1.0);
    let height = room.y.max(1.0) / page.height.max(1.0);
    match fit {
        Fit::Width => width,
        Fit::Page => width.min(height),
    }
}

/// Smaller rooms are a pane mid-collapse: no zoom is measured there.
const MIN_ROOM: f32 = 64.0;

/// A zoom set by hand, kept: it scales as its fit would, up to itself.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Kept {
    /// The zoom set: the most it ever shows.
    zoom: f32,
    /// The basis fit's zoom when it was set.
    span: f32,
}

impl Kept {
    /// The zoom for a fit of `span` now: shrunk by the same ratio.
    fn at(self, span: f32) -> f32 {
        let scaled = self.zoom * span / self.span;
        if scaled.is_finite() {
            clamp(scaled.min(self.zoom))
        } else {
            clamp(self.zoom)
        }
    }
}

/// Whether two zooms differ enough to lay out again.
pub fn differs(a: f32, b: f32) -> bool {
    (a - b).abs() > 1e-4
}

/// A zoom step's glide, seconds; linear, as in mareader.
const GLIDE: f64 = 0.12;
/// Quiet after a pinch or resize before pages render sharp again.
const SETTLE: f64 = 0.18;

/// A zoom step in flight: it glides from `from` to the target.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Glide {
    from: f32,
    start: f64,
}

/// One pane's zoom: what was asked, what shows, what renders.
#[derive(Debug, Clone, PartialEq)]
pub struct PaneZoom {
    /// While set, the zoom follows the pane's size.
    pub fit: Option<Fit>,
    /// The page a fit measures: the one read when it was chosen.
    pub fit_page: usize,
    /// The zoom asked for; a fit recomputes it every frame.
    target: f32,
    glide: Option<Glide>,
    /// Pages render at this zoom; it trails pinches until they settle.
    sharp: f32,
    settle_at: f64,
    /// Where a pinch pivots, in the viewport; else its centre.
    pub pivot: Option<Vec2>,
    /// The room last measured, and when it last changed.
    room: Vec2,
    room_at: f64,
    /// The fit a zoom set by hand scales by: the last one used.
    basis: Fit,
    /// A zoom set by hand while kept zooms scale; none until measured.
    kept: Option<Kept>,
}

impl Default for PaneZoom {
    fn default() -> Self {
        Self {
            fit: Some(Fit::Width),
            fit_page: 0,
            target: 1.0,
            glide: None,
            sharp: 1.0,
            settle_at: 0.0,
            pivot: None,
            room: Vec2::ZERO,
            room_at: 0.0,
            basis: Fit::Width,
            kept: None,
        }
    }
}

impl PaneZoom {
    pub fn state(&self) -> ZoomState {
        ZoomState {
            zoom: self.target,
            fit: self.fit,
        }
    }

    /// Steps glide from `shown` about the centre and render sharp at once.
    pub fn command(&mut self, command: ZoomCommand, shown: Option<f32>, page: usize, now: f64) {
        match command {
            ZoomCommand::Step(step) => {
                let Some(zoom) = next(self.target, step) else {
                    return;
                };
                self.target = zoom;
                self.fit = None;
                self.fit_page = page;
            }
            ZoomCommand::Fit(fit) => {
                self.fit = Some(fit);
                self.fit_page = page;
            }
        }
        self.kept = None;
        self.glide = shown.map(|from| Glide { from, start: now });
        self.pivot = None;
        self.settle_at = now;
    }

    /// Keeps the zoom a fit reached; the pane stops choosing it.
    pub fn unfit(&mut self) {
        self.fit = None;
        self.kept = None;
    }

    /// The fit a set zoom scales by: the strip sideways scales by height.
    pub fn set_basis(&mut self, basis: Fit) {
        self.basis = basis;
        self.kept = None;
    }

    /// What the room is measured for: the fit, or a set zoom's basis.
    pub fn measure(&self) -> Fit {
        self.fit.unwrap_or(self.basis)
    }

    /// Remembers a set zoom for `keep`, measured on `page` in the last room.
    pub fn hold(&mut self, keep: bool, page: Option<PageSize>) {
        if self.fit.is_some() || !keep {
            self.kept = None;
            return;
        }
        let room = self.room;
        let (None, Some(page), true) = (self.kept, page, room.min_elem() >= MIN_ROOM) else {
            return;
        };
        let span = span(self.basis, room, page);
        if span.is_finite() && span > 0.0 {
            self.kept = Some(Kept {
                zoom: self.target,
                span,
            });
        }
    }

    /// A pinch or Ctrl+wheel: follows the fingers, pivots on the pointer.
    pub fn pinch(&mut self, factor: f32, at: Vec2, now: f64) {
        self.target = clamp(self.target * factor);
        self.fit = None;
        self.kept = None;
        self.glide = None;
        self.pivot = Some(at);
        self.settle_at = now + SETTLE;
    }

    /// A fit, or a kept zoom, follows the pane; else returns the wait.
    pub fn refit(
        &mut self,
        page: Option<PageSize>,
        room: Vec2,
        shown: bool,
        follow: bool,
        now: f64,
    ) -> Option<f32> {
        if self.room != room {
            self.room = room;
            self.room_at = now;
        }
        let page = page?;
        if self.fit.is_none() && self.kept.is_none() {
            return None;
        }
        let quiet = now - self.room_at;
        if !follow && shown && quiet < SETTLE {
            return Some((SETTLE - quiet) as f32);
        }
        let fitted = match (self.fit, self.kept) {
            (Some(mode), _) => {
                self.basis = mode;
                fit(mode, room, page)
            }
            (None, Some(kept)) if room.min_elem() >= MIN_ROOM => {
                kept.at(span(self.basis, room, page))
            }
            // Mid-collapse the zoom holds; it comes back with the room.
            _ => return None,
        };
        if differs(fitted, self.target) && self.glide.is_none() && shown {
            self.settle_at = self.settle_at.max(now + SETTLE);
        }
        self.target = fitted;
        None
    }

    /// This frame's zoom, advancing a glide; true while one runs.
    pub fn advance(&mut self, now: f64) -> (f32, bool) {
        let Some(g) = self.glide else {
            return (self.target, false);
        };
        let t = ((now - g.start) / GLIDE) as f32;
        if !(0.0..1.0).contains(&t) {
            self.glide = None;
            return (self.target, false);
        }
        (g.from + (self.target - g.from) * t, true)
    }

    /// Moves the render zoom to the target once settled; else the wait.
    pub fn sharpen(&mut self, now: f64) -> Option<f32> {
        if now >= self.settle_at {
            self.sharp = self.target;
            None
        } else {
            Some((self.settle_at - now) as f32)
        }
    }

    pub fn sharp(&self) -> f32 {
        self.sharp
    }

    /// A pinch's pivot lasts until the zoom comes to rest.
    pub fn end_frame(&mut self, gliding: bool, now: f64) {
        if !gliding && now >= self.settle_at {
            self.pivot = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LETTER: PageSize = PageSize {
        width: 612.0,
        height: 792.0,
    };

    #[test]
    fn steps_climb_to_the_next_preset_and_stop_at_the_ends() {
        assert_eq!(next(1.0, Step::In), Some(1.25));
        assert_eq!(next(1.0, Step::Out), Some(0.9));
        // Off the ladder, the nearest preset that way, never a skip.
        assert_eq!(next(1.2, Step::In), Some(1.25));
        assert_eq!(next(1.2, Step::Out), Some(1.0));
        assert_eq!(next(MAX, Step::In), None);
        assert_eq!(next(MIN, Step::Out), None);
        // Float noise from a fit is still the preset itself.
        assert_eq!(next(1.249, Step::In), Some(1.5));
    }

    #[test]
    fn chained_steps_walk_the_whole_ladder() {
        let mut zoom = MIN;
        let mut seen = vec![zoom];
        while let Some(z) = next(zoom, Step::In) {
            zoom = z;
            seen.push(z);
        }
        assert_eq!(seen, PRESETS);
    }

    #[test]
    fn fit_page_takes_the_tighter_side() {
        let room = Vec2::new(612.0, 2000.0);
        assert!((fit(Fit::Width, room, LETTER) - 1.0).abs() < 1e-6);
        assert!((fit(Fit::Page, room, LETTER) - 1.0).abs() < 1e-6);
        let short = Vec2::new(612.0, 396.0);
        assert!((fit(Fit::Page, short, LETTER) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn fits_clamp_and_garbage_does_not_leak() {
        assert_eq!(fit(Fit::Width, Vec2::new(10.0, 10.0), LETTER), MIN);
        assert_eq!(fit(Fit::Width, Vec2::new(1e6, 10.0), LETTER), MAX);
        assert_eq!(clamp(f32::NAN), MIN);
        let state = ZoomState {
            zoom: 1.25,
            fit: None,
        };
        assert_eq!(state.percent(), "125%");
        assert!(
            !ZoomState {
                zoom: MAX,
                fit: None
            }
            .can_step(Step::In)
        );
    }

    #[test]
    fn steps_glide_and_leave_the_fit() {
        let mut z = PaneZoom::default();
        z.command(ZoomCommand::Step(Step::In), Some(1.0), 0, 10.0);
        assert_eq!(
            z.state(),
            ZoomState {
                zoom: 1.25,
                fit: None
            }
        );
        let (mid, gliding) = z.advance(10.0 + GLIDE / 2.0);
        assert!(gliding && (mid - 1.125).abs() < 1e-3, "{mid}");
        assert_eq!(z.advance(10.0 + GLIDE), (1.25, false));
        // A step renders sharp straight away.
        assert_eq!(z.sharpen(10.0), None);
        assert_eq!(z.sharp(), 1.25);
        z.command(ZoomCommand::Fit(Fit::Page), Some(1.25), 4, 11.0);
        assert_eq!((z.fit, z.fit_page), (Some(Fit::Page), 4));
    }

    #[test]
    fn a_pinch_stretches_until_it_settles() {
        let mut z = PaneZoom::default();
        z.pinch(1.1, Vec2::new(10.0, 10.0), 3.0);
        assert!((z.target - 1.1).abs() < 1e-6);
        assert_eq!(z.fit, None);
        assert!(z.sharpen(3.05).is_some());
        assert_eq!(z.sharp(), 1.0);
        assert_eq!(z.sharpen(3.0 + SETTLE), None);
        assert!((z.sharp() - 1.1).abs() < 1e-6);
        z.pinch(100.0, Vec2::ZERO, 4.0);
        assert_eq!(z.target, MAX);
    }

    #[test]
    fn a_fit_follows_the_room() {
        let mut z = PaneZoom::default();
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), false, true, 0.0);
        assert!((z.state().zoom - 1.0).abs() < 1e-6);
        // A resize refits now and sharpens later.
        z.refit(Some(LETTER), Vec2::new(1224.0, 900.0), true, true, 1.0);
        assert!((z.state().zoom - 2.0).abs() < 1e-6);
        assert!(z.sharpen(1.0).is_some());
    }

    fn near(z: &PaneZoom, want: f32) -> bool {
        (z.state().zoom - want).abs() < 1e-4
    }

    /// A pane fitted to LETTER's width in `room`, then stepped out to 90 %.
    fn set_at_90(room: Vec2) -> PaneZoom {
        let mut z = PaneZoom::default();
        z.refit(Some(LETTER), room, false, true, 0.0);
        z.command(ZoomCommand::Step(Step::Out), None, 0, 1.0);
        z.hold(true, Some(LETTER));
        assert!(near(&z, 0.9));
        z
    }

    #[test]
    fn a_kept_zoom_shrinks_like_a_fit_and_grows_back_only_to_itself() {
        let room = Vec2::new(612.0, 900.0);
        let mut z = set_at_90(room);
        let at = |z: &mut PaneZoom, x: f32| {
            z.refit(Some(LETTER), Vec2::new(x, 900.0), true, true, 2.0);
            z.state().zoom
        };
        // Half the pane, half the zoom: as fit width halves.
        assert!((at(&mut z, 306.0) - 0.45).abs() < 1e-4);
        assert_eq!(z.state().fit, None, "still set by hand");
        // Wider than it was set in, it stops at what was set.
        assert!((at(&mut z, 1224.0) - 0.9).abs() < 1e-4);
        assert!((at(&mut z, 612.0) - 0.9).abs() < 1e-4);
        // A pane mid-collapse is no measure; the zoom waits for the room.
        assert!((at(&mut z, 20.0) - 0.9).abs() < 1e-4);
        assert!((at(&mut z, 459.0) - 0.675).abs() < 1e-4);
        assert!((at(&mut z, 612.0) - 0.9).abs() < 1e-4);
    }

    #[test]
    fn a_new_zoom_replaces_the_kept_one() {
        let mut z = set_at_90(Vec2::new(612.0, 900.0));
        let half = Vec2::new(306.0, 900.0);
        z.refit(Some(LETTER), half, true, true, 2.0);
        assert!(near(&z, 0.45));
        // Stepping steps from what shows, and that is kept now.
        z.command(ZoomCommand::Step(Step::In), None, 0, 3.0);
        assert!(near(&z, 0.5));
        z.hold(true, Some(LETTER));
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), true, true, 4.0);
        assert!(near(&z, 0.5), "never past the new one");
        z.refit(Some(LETTER), Vec2::new(153.0, 900.0), true, true, 5.0);
        assert!(near(&z, 0.25));
        // A pinch replaces it too, from what shows.
        z.pinch(2.0, Vec2::ZERO, 6.0);
        z.hold(true, Some(LETTER));
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), true, true, 7.0);
        assert!(near(&z, 0.5));
    }

    #[test]
    fn a_kept_zoom_scales_down_on_wider_pages_never_up() {
        let mut z = set_at_90(Vec2::new(612.0, 900.0));
        let wide = PageSize {
            width: 792.0,
            height: 612.0,
        };
        let narrow = PageSize {
            width: 400.0,
            height: 600.0,
        };
        let room = Vec2::new(612.0, 900.0);
        z.refit(Some(wide), room, true, true, 2.0);
        assert!(near(&z, 0.9 * 612.0 / 792.0));
        z.refit(Some(narrow), room, true, true, 3.0);
        assert!(near(&z, 0.9));
    }

    #[test]
    fn unkept_a_set_zoom_holds_where_it_shows() {
        let mut z = set_at_90(Vec2::new(612.0, 900.0));
        z.refit(Some(LETTER), Vec2::new(306.0, 900.0), true, true, 2.0);
        // Turned off: no jump back, and no scaling after.
        z.hold(false, Some(LETTER));
        z.refit(Some(LETTER), Vec2::new(1224.0, 900.0), true, true, 3.0);
        assert!(near(&z, 0.45));
        // On again, what shows is kept, measured in this room.
        z.hold(true, Some(LETTER));
        z.refit(Some(LETTER), Vec2::new(1224.0, 1800.0), true, true, 4.0);
        assert!(near(&z, 0.45), "fit width ignores the height");
        z.refit(Some(LETTER), Vec2::new(816.0, 900.0), true, true, 5.0);
        assert!(near(&z, 0.3));
    }

    #[test]
    fn a_set_zoom_scales_by_its_basis_fit() {
        let mut z = PaneZoom::default();
        z.command(ZoomCommand::Fit(Fit::Page), None, 0, 0.0);
        // Fit page in a short pane: the height decides.
        z.refit(Some(LETTER), Vec2::new(1224.0, 792.0), false, true, 0.0);
        assert!(near(&z, 1.0));
        z.command(ZoomCommand::Step(Step::Out), None, 0, 1.0);
        z.hold(true, Some(LETTER));
        // Narrower but still wide enough: fit page would not move.
        z.refit(Some(LETTER), Vec2::new(900.0, 792.0), true, true, 2.0);
        assert!(near(&z, 0.9));
        z.refit(Some(LETTER), Vec2::new(1224.0, 396.0), true, true, 3.0);
        assert!(near(&z, 0.45));
    }

    #[test]
    fn without_follow_a_fit_waits_for_the_resize_to_rest() {
        let mut z = PaneZoom::default();
        z.refit(Some(LETTER), Vec2::new(612.0, 900.0), false, false, 0.0);
        let wide = Vec2::new(1224.0, 900.0);
        // Still dragging: the zoom holds, and says how long to wait.
        assert!(z.refit(Some(LETTER), wide, true, false, 1.0).is_some());
        assert!((z.state().zoom - 1.0).abs() < 1e-6);
        assert!(z.refit(Some(LETTER), wide, true, false, 1.1).is_some());
        // At rest: one fit.
        let rested = 1.01 + SETTLE;
        assert_eq!(z.refit(Some(LETTER), wide, true, false, rested), None);
        assert!((z.state().zoom - 2.0).abs() < 1e-6);
    }
}
