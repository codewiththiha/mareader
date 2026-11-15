/// A unit-interval damped spring, matched to the lab's 60 Hz evaluator.
pub(crate) fn evaluate(zeta: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let z = zeta.clamp(0.05, 0.999);
    let w = 6.2 / z;
    let wd = w * (1.0 - z * z).sqrt();
    let raw = |t: f32| 1.0 - (-z * w * t).exp() * ((wd * t).cos() + (z * w / wd) * (wd * t).sin());
    raw(x) + (1.0 - raw(1.0)) * x
}
