//! Loops built again for AVX2 and FMA; kernels inline, so each build has one.

/// `kernel(args)`, built for AVX2 and FMA if the CPU has them.
#[inline(always)]
pub fn pick<A, R>(kernel: impl FnOnce(A) -> R, args: A) -> R {
    #[cfg(target_arch = "x86_64")]
    if wide() {
        return avx2(kernel, args);
    }
    kernel(args)
}

/// Whether this CPU runs AVX2 and FMA; std caches the answer.
#[cfg(target_arch = "x86_64")]
fn wide() -> bool {
    std::arch::is_x86_feature_detected!("avx2") && std::arch::is_x86_feature_detected!("fma")
}

/// The AVX2 build; only `pick` calls it, once `wide` said yes.
#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
fn avx2<A, R>(kernel: impl FnOnce(A) -> R, args: A) -> R {
    #[target_feature(enable = "avx2,fma")]
    fn built<A, R>(kernel: impl FnOnce(A) -> R, args: A) -> R {
        kernel(args)
    }
    // SAFETY: `wide` found AVX2 and FMA on this CPU before this call.
    unsafe { built(kernel, args) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[inline(always)]
    fn dot((a, b): (&[f32], &[f32])) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    fn either_build_gives_the_same_answer() {
        let a: Vec<f32> = (0..1000).map(|i| i as f32 * 0.25).collect();
        let b: Vec<f32> = (0..1000).map(|i| (i % 7) as f32).collect();
        assert_eq!(pick(dot, (&a, &b)), dot((&a, &b)));
    }
}
