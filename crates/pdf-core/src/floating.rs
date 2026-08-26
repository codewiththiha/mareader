//! Floating UI geometry: generic placement / clamping math for anchored
//! panels, context menus, toasts and floating cards. Pure — no DOM, no
//! leptos — so it is unit-testable on the host via
//! `cargo test -p pdf-core floating`.
//!
//! This is the "mechanism" half of the floating system that lives in
//! `src/components/primitives/floating`: placement *policy* (which side a
//! panel prefers, what it contains) is decided by the callers; the math here
//! only answers "given this anchor and this panel, where does it go, and is
//! it inside the viewport?"

/// A plain 2-D size in CSS px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Size {
    pub const fn new(w: f64, h: f64) -> Self {
        Self { w, h }
    }
}

/// A point in the viewport (CSS px).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A positioned rect.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(self) -> f64 {
        self.y + self.h
    }
    pub fn top(self) -> f64 {
        self.y
    }
    pub fn left(self) -> f64 {
        self.x
    }
    /// Vertical center of the rect.
    pub fn center_y(self) -> f64 {
        self.y + self.h * 0.5
    }
}

/// The five-field box the spring drives (position + size + corner radius).
/// Named to avoid colliding with `std::boxed::Box`; `pdf_core::gloss::GlossBox`
/// converts into/out of this type at the domain boundary.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
pub struct FloatBox {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Corner radius — animated alongside the box so a pill can morph into a
    /// card radius in the same motion that grows the box.
    pub r: f64,
}

impl FloatBox {
    /// One explicit-Euler spring step over all five fields. Returns
    /// `(next_box, next_velocity)`. dt is clamped by the caller so long
    /// frames never blow the integrator past its stability bound.
    pub fn step(&self, vel: &FloatBox, target: &FloatBox, dt: f64) -> (FloatBox, FloatBox) {
        let (x, vx) = spring_axis(self.x, vel.x, target.x, dt);
        let (y, vy) = spring_axis(self.y, vel.y, target.y, dt);
        let (w, vw) = spring_axis(self.w, vel.w, target.w, dt);
        let (h, vh) = spring_axis(self.h, vel.h, target.h, dt);
        let (r, vr) = spring_axis(self.r, vel.r, target.r, dt);
        (
            FloatBox { x, y, w, h, r },
            FloatBox {
                x: vx,
                y: vy,
                w: vw,
                h: vh,
                r: vr,
            },
        )
    }

    /// Whether two boxes are equal to within `epsilon` on all five fields.
    pub fn close(&self, other: &FloatBox, epsilon: f64) -> bool {
        (self.x - other.x).abs() < epsilon
            && (self.y - other.y).abs() < epsilon
            && (self.w - other.w).abs() < epsilon
            && (self.h - other.h).abs() < epsilon
            && (self.r - other.r).abs() < epsilon
    }

    /// Whether every field is below `epsilon` in magnitude — the spring's
    /// "settled" test.
    pub fn all_small(&self, epsilon: f64) -> bool {
        self.x.abs() < epsilon
            && self.y.abs() < epsilon
            && self.w.abs() < epsilon
            && self.h.abs() < epsilon
            && self.r.abs() < epsilon
    }
}

/// Spring stiffness and damping for animated boxes. Stiffness 210 / damping
/// 26 is mildly underdamped (critical ≈ 29 at mass 1): a confident pop with
/// one small settle.
pub const SPRING_STIFFNESS: f64 = 210.0;
pub const SPRING_DAMPING: f64 = 26.0;

/// One explicit-Euler step of a 1-D spring toward `t` from `c` at velocity
/// `v`. Returns `(position, velocity)`.
pub fn spring_axis(c: f64, v: f64, t: f64, dt: f64) -> (f64, f64) {
    let force = SPRING_STIFFNESS * (t - c) - SPRING_DAMPING * v;
    let nv = v + force * dt;
    (c + nv * dt, nv)
}

/// Which side of the anchor the panel prefers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlacementSide {
    /// Pick the side with room: below the anchor, flipping above when the
    /// bottom would overflow (the classic menu behaviour).
    #[default]
    Auto,
    Below,
    Above,
    Left,
    Right,
}

/// Placement inputs for [`place_panel_from_anchor`].
#[derive(Debug, Clone, Copy)]
pub struct PlacementOptions {
    pub side: PlacementSide,
    /// Gap between the anchor and the panel's near edge.
    pub gap: f64,
    /// Min distance from the viewport edges.
    pub margin: f64,
    /// The viewport the panel must stay inside.
    pub viewport: Size,
}

/// A placed panel: the final rect plus the CSS `transform-origin` that makes
/// a scale-in animation emerge from the anchor edge.
#[derive(Debug, Clone, Copy)]
pub struct PlacedPanel {
    pub rect: Rect,
    pub transform_origin: &'static str,
}

/// Clamp `p` so a box of `size` stays inside `viewport` with `margin`.
pub fn clamp_point_to_viewport(p: Point, size: Size, viewport: Size, margin: f64) -> Point {
    let max_x = (viewport.w - size.w - margin).max(margin);
    let max_y = (viewport.h - size.h - margin).max(margin);
    Point {
        x: p.x.clamp(margin, max_x),
        y: p.y.clamp(margin, max_y),
    }
}

/// Clamp `rect` inside the viewport margin, shrinking the allowed range when
/// the panel is larger than the viewport allows (the range collapses to the
/// margin rather than panicking on min > max).
pub fn clamp_rect_to_viewport(rect: Rect, viewport: Size, margin: f64) -> Rect {
    Rect {
        x: rect.x.clamp(margin, (viewport.w - rect.w - margin).max(margin)),
        y: rect.y.clamp(margin, (viewport.h - rect.h - margin).max(margin)),
        ..rect
    }
}

/// Right-aligned below/above placement for anchored menu panels.
///
/// `Auto` opens below the anchor and flips above when the panel would
/// overflow the bottom edge; the panel is right-aligned to the anchor and
/// clamped into the viewport. Left/Right placements centre vertically on the
/// anchor instead. The returned `transform_origin` matches the side the
/// panel actually opened on.
pub fn place_panel_from_anchor(anchor: Rect, panel: Size, opts: &PlacementOptions) -> PlacedPanel {
    let m = opts.margin;
    let gap = opts.gap;
    let vp = opts.viewport;
    let fits_below = anchor.bottom() + gap + panel.h <= vp.h - m;

    let below = PlacedPanel {
        rect: Rect::new(anchor.right() - panel.w, anchor.bottom() + gap, panel.w, panel.h),
        transform_origin: "top right",
    };
    let above = PlacedPanel {
        rect: Rect::new(anchor.right() - panel.w, anchor.top() - gap - panel.h, panel.w, panel.h),
        transform_origin: "bottom right",
    };

    let placed = match opts.side {
        PlacementSide::Below => below,
        PlacementSide::Above => above,
        PlacementSide::Auto if fits_below => below,
        PlacementSide::Auto => above,
        PlacementSide::Left => PlacedPanel {
            rect: Rect::new(
                anchor.x - gap - panel.w,
                anchor.center_y() - panel.h * 0.5,
                panel.w,
                panel.h,
            ),
            transform_origin: "right center",
        },
        PlacementSide::Right => PlacedPanel {
            rect: Rect::new(anchor.right() + gap, anchor.center_y() - panel.h * 0.5, panel.w, panel.h),
            transform_origin: "left center",
        },
    };
    PlacedPanel {
        rect: clamp_rect_to_viewport(placed.rect, vp, m),
        transform_origin: placed.transform_origin,
    }
}

/// Clamp a cursor point so a context menu of `panel` size stays inside the
/// viewport margin. `panel` should be the menu's *measured* size; callers
/// fall back to a small guess before the menu has mounted.
pub fn place_context_menu(point: Point, panel: Size, viewport: Size, margin: f64) -> PlacedPanel {
    let p = clamp_point_to_viewport(point, panel, viewport, margin);
    PlacedPanel {
        rect: Rect::new(p.x, p.y, panel.w, panel.h),
        transform_origin: "top left",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_point_keeps_a_box_inside_the_margin() {
        let vp = Size::new(800.0, 600.0);
        let p = clamp_point_to_viewport(Point::new(790.0, 590.0), Size::new(100.0, 80.0), vp, 8.0);
        assert!(p.x <= 800.0 - 100.0 - 8.0 + 1e-9);
        assert!(p.y <= 600.0 - 80.0 - 8.0 + 1e-9);
        let p = clamp_point_to_viewport(Point::new(0.0, 0.0), Size::new(100.0, 80.0), vp, 8.0);
        assert!(p.x >= 8.0 - 1e-9 && p.y >= 8.0 - 1e-9);
    }

    #[test]
    fn clamp_rect_never_panics_on_an_oversized_panel() {
        // A panel bigger than the viewport collapses the range to the margin.
        let r = clamp_rect_to_viewport(Rect::new(0.0, 0.0, 2000.0, 2000.0), Size::new(500.0, 400.0), 12.0);
        assert!(r.x.is_finite() && r.y.is_finite());
        assert!(r.x >= 12.0 - 1e-9 && r.y >= 12.0 - 1e-9);
    }

    #[test]
    fn place_panel_right_aligns_below_the_anchor() {
        let anchor = Rect::new(300.0, 100.0, 120.0, 32.0);
        let opts = PlacementOptions {
            side: PlacementSide::Auto,
            gap: 4.0,
            margin: 8.0,
            viewport: Size::new(1280.0, 800.0),
        };
        let placed = place_panel_from_anchor(anchor, Size::new(256.0, 200.0), &opts);
        assert!((placed.rect.x - (anchor.right() - 256.0)).abs() < 1e-9);
        assert!((placed.rect.y - (anchor.bottom() + 4.0)).abs() < 1e-9);
        assert_eq!(placed.transform_origin, "top right");
    }

    #[test]
    fn place_panel_flips_above_when_the_bottom_overflows() {
        let anchor = Rect::new(300.0, 700.0, 120.0, 32.0);
        let opts = PlacementOptions {
            side: PlacementSide::Auto,
            gap: 4.0,
            margin: 8.0,
            viewport: Size::new(1280.0, 800.0),
        };
        let placed = place_panel_from_anchor(anchor, Size::new(256.0, 200.0), &opts);
        assert!((placed.rect.y - (anchor.top() - 4.0 - 200.0)).abs() < 1e-9);
        assert_eq!(placed.transform_origin, "bottom right");
    }

    #[test]
    fn place_panel_clamps_into_the_viewport_margin() {
        let anchor = Rect::new(1200.0, 20.0, 60.0, 30.0);
        let opts = PlacementOptions {
            side: PlacementSide::Auto,
            gap: 4.0,
            margin: 8.0,
            viewport: Size::new(1280.0, 800.0),
        };
        let placed = place_panel_from_anchor(anchor, Size::new(256.0, 300.0), &opts);
        assert!(placed.rect.x >= 8.0 - 1e-9);
        assert!(placed.rect.x + placed.rect.w <= 1280.0 - 8.0 + 1e-6);
    }

    #[test]
    fn place_context_menu_stays_inside_the_viewport() {
        let p = place_context_menu(Point::new(1270.0, 790.0), Size::new(176.0, 60.0), Size::new(1280.0, 800.0), 8.0);
        assert!(p.rect.x + p.rect.w <= 1280.0 - 8.0 + 1e-6);
        assert!(p.rect.y + p.rect.h <= 800.0 - 8.0 + 1e-6);
        assert_eq!(p.transform_origin, "top left");
    }

    #[test]
    fn the_spring_converges_within_about_two_seconds() {
        let target = FloatBox { x: 200.0, y: 150.0, w: 320.0, h: 420.0, r: 24.0 };
        let mut cur = FloatBox { x: 40.0, y: 30.0, w: 30.0, h: 20.0, r: 10.0 };
        let mut vel = FloatBox::default();
        for _ in 0..200 {
            let (next, next_vel) = cur.step(&vel, &target, 1.0 / 60.0);
            cur = next;
            vel = next_vel;
        }
        assert!(cur.close(&target, 0.5), "did not settle: {cur:?}");
    }

    #[test]
    fn the_spring_is_stable_on_a_dropped_frame() {
        let target = FloatBox { x: 0.0, y: 0.0, w: 300.0, h: 300.0, r: 20.0 };
        let mut cur = FloatBox::default();
        let mut vel = FloatBox::default();
        for _ in 0..400 {
            let (next, next_vel) = cur.step(&vel, &target, 0.032);
            cur = next;
            vel = next_vel;
            assert!(cur.x.is_finite() && cur.w.is_finite(), "blew up: {cur:?}");
        }
        assert!(cur.close(&target, 0.5), "did not settle on long frames: {cur:?}");
    }
}
