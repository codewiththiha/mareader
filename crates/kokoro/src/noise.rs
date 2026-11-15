//! Seeded Gaussian noise, so the same phonemes always sound the same.

use mareader_nn_core::noise;

/// SplitMix64 numbers, paired into normal draws by Box–Muller.
pub(crate) struct Noise {
    state: u64,
    spare: Option<f32>,
    gain: f32,
}

impl Noise {
    pub(crate) fn new(seed: u64) -> Self {
        Self {
            state: seed,
            spare: None,
            gain: 1.0,
        }
    }

    /// Every draw scaled by `gain`; zero makes the source noiseless.
    pub(crate) fn with_gain(self, gain: f32) -> Self {
        Self { gain, ..self }
    }

    /// A seed drawn from token ids, so equal input gives equal sound.
    pub(crate) fn seed_for(ids: &[u32]) -> u64 {
        ids.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &id| {
            (h ^ u64::from(id)).wrapping_mul(0x0100_0000_01B3)
        })
    }

    /// Uniform in `(0, 1]`, never zero so the log stays finite.
    fn unit(&mut self) -> f64 {
        ((noise::next(&mut self.state) >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// `out` filled as calls to `normal` would, the maths across cores.
    pub(crate) fn fill_normal(&mut self, out: &mut [f32]) {
        let gain = self.gain;
        let mut rest = out;
        if let Some(v) = self.spare.take() {
            let Some((first, tail)) = rest.split_first_mut() else {
                self.spare = Some(v);
                return;
            };
            *first = v * gain;
            rest = tail;
        }
        let paired = rest.len() / 2 * 2;
        let units: Vec<f64> = (0..paired).map(|_| self.unit()).collect();
        let (pairs, odd) = rest.split_at_mut(paired);
        crate::fast::by_rows(pairs, 2, |i, pair| {
            let (Some(u), Some(w), [cos, sin]) = (units.get(2 * i), units.get(2 * i + 1), pair)
            else {
                return;
            };
            let radius = (-2.0 * u.ln()).sqrt();
            let angle = std::f64::consts::TAU * w;
            *cos = (radius * angle.cos()) as f32 * gain;
            *sin = (radius * angle.sin()) as f32 * gain;
        });
        if let Some(last) = odd.first_mut() {
            *last = self.normal();
        }
    }

    /// One draw from the standard normal distribution.
    pub(crate) fn normal(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v * self.gain;
        }
        let radius = (-2.0 * self.unit().ln()).sqrt();
        let angle = std::f64::consts::TAU * self.unit();
        self.spare = Some((radius * angle.sin()) as f32);
        (radius * angle.cos()) as f32 * self.gain
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_repeat_per_seed_and_look_normal() {
        let a: Vec<f32> = (0..4)
            .map({
                let mut n = Noise::new(7);
                move |_| n.normal()
            })
            .collect();
        let b: Vec<f32> = (0..4)
            .map({
                let mut n = Noise::new(7);
                move |_| n.normal()
            })
            .collect();
        assert_eq!(a, b);
        let mut n = Noise::new(Noise::seed_for(&[1, 2, 3]));
        let draws: Vec<f64> = (0..20_000).map(|_| f64::from(n.normal())).collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let var = draws.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / draws.len() as f64;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "variance {var}");
    }

    #[test]
    fn filling_draws_what_single_draws_would() {
        for (skip, count) in [(0, 70_001), (1, 70_000), (3, 1), (1, 0)] {
            let mut one = Noise::new(9).with_gain(0.5);
            let mut many = Noise::new(9).with_gain(0.5);
            for _ in 0..skip {
                assert_eq!(one.normal(), many.normal());
            }
            let singles: Vec<f32> = (0..count).map(|_| one.normal()).collect();
            let mut filled = vec![0f32; count];
            many.fill_normal(&mut filled);
            assert_eq!(singles, filled);
            assert_eq!(one.normal(), many.normal());
        }
    }
}
