//! Dot product kernels (plan §5.3). Vectors are L2-normalized so the dot
//! product is the cosine similarity. AVX2+FMA on x86_64 and NEON on
//! aarch64 are selected at runtime; everything else takes the scalar path.
//! Kernels are checked against the scalar version in tests.

#[inline]
pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            // SAFETY: feature detection succeeded; slices are equal length.
            return unsafe { dot_avx2(a, b) };
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        // NEON is baseline on aarch64.
        // SAFETY: slices are equal length.
        return unsafe { dot_neon(a, b) };
    }
    #[allow(unreachable_code)]
    dot_scalar(a, b)
}

pub fn dot_scalar(a: &[f32], b: &[f32]) -> f32 {
    // Four accumulators: lets the compiler vectorize and keeps the
    // summation order close to the SIMD paths.
    let mut acc = [0f32; 4];
    let chunks = a.len() / 4;
    for i in 0..chunks {
        let o = i * 4;
        acc[0] += a[o] * b[o];
        acc[1] += a[o + 1] * b[o + 1];
        acc[2] += a[o + 2] * b[o + 2];
        acc[3] += a[o + 3] * b[o + 3];
    }
    let mut s = acc[0] + acc[1] + acc[2] + acc[3];
    for i in chunks * 4..a.len() {
        s += a[i] * b[i];
    }
    s
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2,fma")]
unsafe fn dot_avx2(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::x86_64::*;
    let n = a.len();
    let mut acc0 = _mm256_setzero_ps();
    let mut acc1 = _mm256_setzero_ps();
    let mut i = 0;
    while i + 16 <= n {
        let a0 = _mm256_loadu_ps(a.as_ptr().add(i));
        let b0 = _mm256_loadu_ps(b.as_ptr().add(i));
        let a1 = _mm256_loadu_ps(a.as_ptr().add(i + 8));
        let b1 = _mm256_loadu_ps(b.as_ptr().add(i + 8));
        acc0 = _mm256_fmadd_ps(a0, b0, acc0);
        acc1 = _mm256_fmadd_ps(a1, b1, acc1);
        i += 16;
    }
    while i + 8 <= n {
        let a0 = _mm256_loadu_ps(a.as_ptr().add(i));
        let b0 = _mm256_loadu_ps(b.as_ptr().add(i));
        acc0 = _mm256_fmadd_ps(a0, b0, acc0);
        i += 8;
    }
    let acc = _mm256_add_ps(acc0, acc1);
    let hi = _mm256_extractf128_ps(acc, 1);
    let lo = _mm256_castps256_ps128(acc);
    let s = _mm_add_ps(hi, lo);
    let s = _mm_hadd_ps(s, s);
    let s = _mm_hadd_ps(s, s);
    let mut total = _mm_cvtss_f32(s);
    while i < n {
        total += a[i] * b[i];
        i += 1;
    }
    total
}

#[cfg(target_arch = "aarch64")]
unsafe fn dot_neon(a: &[f32], b: &[f32]) -> f32 {
    use std::arch::aarch64::*;
    let n = a.len();
    let mut acc0 = vdupq_n_f32(0.0);
    let mut acc1 = vdupq_n_f32(0.0);
    let mut i = 0;
    while i + 8 <= n {
        acc0 = vfmaq_f32(
            acc0,
            vld1q_f32(a.as_ptr().add(i)),
            vld1q_f32(b.as_ptr().add(i)),
        );
        acc1 = vfmaq_f32(
            acc1,
            vld1q_f32(a.as_ptr().add(i + 4)),
            vld1q_f32(b.as_ptr().add(i + 4)),
        );
        i += 8;
    }
    let mut total = vaddvq_f32(vaddq_f32(acc0, acc1));
    while i < n {
        total += a[i] * b[i];
        i += 1;
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vecs(n: usize, seed: u64) -> (Vec<f32>, Vec<f32>) {
        let mut s = seed | 1;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s % 2000) as f32 / 1000.0 - 1.0
        };
        (
            (0..n).map(|_| next()).collect(),
            (0..n).map(|_| next()).collect(),
        )
    }

    #[test]
    fn kernels_agree_with_scalar_at_odd_lengths() {
        for n in [0, 1, 3, 7, 8, 15, 16, 17, 31, 64, 100, 384, 385] {
            let (a, b) = vecs(n, 42 + n as u64);
            let want = dot_scalar(&a, &b);
            let got = dot(&a, &b);
            assert!(
                (want - got).abs() < 1e-4 * n.max(1) as f32,
                "n={n}: {want} vs {got}"
            );
        }
    }

    #[test]
    fn unit_vectors_give_cosine() {
        let a = vec![1.0, 0.0, 0.0, 0.0];
        let b = vec![0.6, 0.8, 0.0, 0.0];
        assert!((dot(&a, &b) - 0.6).abs() < 1e-6);
        assert!((dot(&b, &b) - 1.0).abs() < 1e-6);
    }
}
