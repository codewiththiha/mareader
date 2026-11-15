//! Seeded Gaussian noise, so the same phonemes always sound the same.

use mareader_nn_core::noise;

/// SplitMix64 numbers, paired into normal draws by Box–Muller.
pub(crate) struct Noise {
    state: u64,
    spare: Option<f32>,
}

impl Noise {
    /// A seed drawn from the ids, so equal input gives equal sound.
    pub(crate) fn for_ids(ids: &[u32]) -> Self {
        let seed = ids.iter().fold(0xCBF2_9CE4_8422_2325u64, |h, &id| {
            (h ^ u64::from(id)).wrapping_mul(0x0100_0000_01B3)
        });
        Self {
            state: seed,
            spare: None,
        }
    }

    /// Uniform in `(0, 1]`, never zero so the log stays finite.
    fn unit(&mut self) -> f64 {
        ((noise::next(&mut self.state) >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    fn normal(&mut self) -> f32 {
        if let Some(v) = self.spare.take() {
            return v;
        }
        let radius = (-2.0 * self.unit().ln()).sqrt();
        let angle = std::f64::consts::TAU * self.unit();
        self.spare = Some((radius * angle.sin()) as f32);
        (radius * angle.cos()) as f32
    }

    /// `count` draws, each times `gain`.
    pub(crate) fn take(&mut self, count: usize, gain: f32) -> Vec<f32> {
        (0..count).map(|_| self.normal() * gain).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draws_repeat_per_input_and_look_normal() {
        assert_eq!(
            Noise::for_ids(&[1, 2]).take(4, 1.0),
            Noise::for_ids(&[1, 2]).take(4, 1.0)
        );
        assert_ne!(
            Noise::for_ids(&[1, 2]).take(4, 1.0),
            Noise::for_ids(&[2, 1]).take(4, 1.0)
        );
        let draws: Vec<f64> = Noise::for_ids(&[7])
            .take(20_000, 1.0)
            .into_iter()
            .map(f64::from)
            .collect();
        let mean = draws.iter().sum::<f64>() / draws.len() as f64;
        let var = draws.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / draws.len() as f64;
        assert!(mean.abs() < 0.03, "mean {mean}");
        assert!((var - 1.0).abs() < 0.05, "variance {var}");
        assert!(Noise::for_ids(&[3]).take(8, 0.0).iter().all(|&v| v == 0.0));
    }
}
