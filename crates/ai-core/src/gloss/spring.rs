//! The gloss box's adapter to the generic spring.
//!
//! `ui_geom::spring` defines the value shape and `ui_kit::motion::spring`
//! drives any [`SpringValue`] without knowing what the fields mean. This is
//! the seam that tells them what a gloss box is: [`super::geometry`] owns the
//! maths, so the adapter is four forwards.
//!
//! It lives beside the type rather than beside the loop because the dependency
//! has to point one way. A generic primitive that imported a feature crate's
//! type could be broken by that crate — and would quietly make every other
//! consumer of the primitive depend on the AI feature too. The trait is public
//! and sits in the leaf both crates already reach, so the type that needs it
//! supplies the adapter.

use super::geometry::GlossBox;
use ui_geom::spring::SpringValue;

impl SpringValue for GlossBox {
    fn zero() -> Self {
        GlossBox::default()
    }
    fn close(&self, other: &Self, epsilon: f64) -> bool {
        super::geometry::boxes_close(*self, *other, epsilon)
    }
    fn step(&self, vel: &Self, target: &Self, dt: f64) -> (Self, Self) {
        super::geometry::step_spring(*self, *vel, *target, dt)
    }
    fn all_small(&self, epsilon: f64) -> bool {
        // A velocity is small exactly when it is close to zero, so this is
        // `boxes_close` against the default rather than a second enumeration of
        // the five fields that could drift from the one `close` uses.
        super::geometry::boxes_close(*self, GlossBox::default(), epsilon)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gloss(x: f64, y: f64, w: f64, h: f64, r: f64) -> GlossBox {
        GlossBox { x, y, w, h, r }
    }

    #[test]
    fn gloss_all_small_covers_every_field() {
        // Each field above epsilon on its own must break "all small", or a
        // still-moving spring would tear its rAF loop down early. This is the
        // only per-field coverage either crate has: `boxes_close`'s own test
        // perturbs all five fields together, which a dropped field survives.
        for above in [
            gloss(1.0, 0.0, 0.0, 0.0, 0.0),
            gloss(0.0, 1.0, 0.0, 0.0, 0.0),
            gloss(0.0, 0.0, 1.0, 0.0, 0.0),
            gloss(0.0, 0.0, 0.0, 1.0, 0.0),
            gloss(0.0, 0.0, 0.0, 0.0, 1.0),
        ] {
            assert!(!above.all_small(0.6), "{above:?} read as small");
        }
        assert!(gloss(0.0, 0.0, 0.0, 0.0, 0.0).all_small(0.6));
    }
}
