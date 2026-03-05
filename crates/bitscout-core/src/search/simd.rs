//! SIMD-accelerated vector operations for Random Projection.
//!
//! Provides three operations with AVX2/NEON/scalar backends:
//! - `weighted_accumulate`: dst[i] += src[i] * weight
//! - `dot_product`: sum(a[i] * b[i])
//! - `norm_sq`: sum(v[i]^2)
//!
//! Runtime detection selects the fastest available path.

// ---------------------------------------------------------------------------
// Public API — runtime dispatch
// ---------------------------------------------------------------------------

/// dst[i] += src[i] * weight for all i.
pub fn weighted_accumulate(dst: &mut [f32], src: &[f32], weight: f32) {
    debug_assert_eq!(dst.len(), src.len());
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe { return avx2::weighted_accumulate_avx2(dst, src, weight); }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { return neon::weighted_accumulate_neon(dst, src, weight); }
    }
    #[allow(unreachable_code)]
    scalar::weighted_accumulate_scalar(dst, src, weight);
}

/// Dot product: sum(a[i] * b[i]).
pub fn dot_product(a: &[f32], b: &[f32]) -> f32 {
    debug_assert_eq!(a.len(), b.len());
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe { return avx2::dot_product_avx2(a, b); }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { return neon::dot_product_neon(a, b); }
    }
    #[allow(unreachable_code)]
    scalar::dot_product_scalar(a, b)
}

/// Squared norm: sum(v[i]^2).
pub fn norm_sq(v: &[f32]) -> f32 {
    #[cfg(target_arch = "x86_64")]
    {
        if is_x86_feature_detected!("avx2") && is_x86_feature_detected!("fma") {
            unsafe { return avx2::norm_sq_avx2(v); }
        }
    }
    #[cfg(target_arch = "aarch64")]
    {
        unsafe { return neon::norm_sq_neon(v); }
    }
    #[allow(unreachable_code)]
    scalar::norm_sq_scalar(v)
}

// ---------------------------------------------------------------------------
// Scalar fallback
// ---------------------------------------------------------------------------

mod scalar {
    pub fn weighted_accumulate_scalar(dst: &mut [f32], src: &[f32], weight: f32) {
        for (d, s) in dst.iter_mut().zip(src.iter()) {
            *d += s * weight;
        }
    }

    pub fn dot_product_scalar(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
    }

    pub fn norm_sq_scalar(v: &[f32]) -> f32 {
        v.iter().map(|x| x * x).sum()
    }
}

// ---------------------------------------------------------------------------
// AVX2 + FMA (x86_64)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "x86_64")]
mod avx2 {
    #[cfg(target_arch = "x86_64")]
    use std::arch::x86_64::*;

    #[target_feature(enable = "avx2,fma")]
    pub unsafe fn weighted_accumulate_avx2(dst: &mut [f32], src: &[f32], weight: f32) {
        let n = dst.len();
        let w = _mm256_set1_ps(weight);
        let chunks = n / 8;

        for i in 0..chunks {
            let offset = i * 8;
            let d = _mm256_loadu_ps(dst.as_ptr().add(offset));
            let s = _mm256_loadu_ps(src.as_ptr().add(offset));
            let result = _mm256_fmadd_ps(s, w, d); // d + s * w
            _mm256_storeu_ps(dst.as_mut_ptr().add(offset), result);
        }

        // Handle remainder
        for i in (chunks * 8)..n {
            *dst.get_unchecked_mut(i) += src.get_unchecked(i) * weight;
        }
    }

    #[target_feature(enable = "avx2,fma")]
    pub unsafe fn dot_product_avx2(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len();
        let chunks = n / 8;
        let mut acc = _mm256_setzero_ps();

        for i in 0..chunks {
            let offset = i * 8;
            let va = _mm256_loadu_ps(a.as_ptr().add(offset));
            let vb = _mm256_loadu_ps(b.as_ptr().add(offset));
            acc = _mm256_fmadd_ps(va, vb, acc); // acc + a * b
        }

        // Horizontal sum of acc
        let mut result = hsum_avx(acc);

        // Handle remainder
        for i in (chunks * 8)..n {
            result += a.get_unchecked(i) * b.get_unchecked(i);
        }

        result
    }

    #[target_feature(enable = "avx2,fma")]
    pub unsafe fn norm_sq_avx2(v: &[f32]) -> f32 {
        let n = v.len();
        let chunks = n / 8;
        let mut acc = _mm256_setzero_ps();

        for i in 0..chunks {
            let offset = i * 8;
            let va = _mm256_loadu_ps(v.as_ptr().add(offset));
            acc = _mm256_fmadd_ps(va, va, acc);
        }

        let mut result = hsum_avx(acc);

        for i in (chunks * 8)..n {
            let x = *v.get_unchecked(i);
            result += x * x;
        }

        result
    }

    /// Horizontal sum of an __m256 register.
    #[target_feature(enable = "avx2")]
    unsafe fn hsum_avx(v: __m256) -> f32 {
        let hi = _mm256_extractf128_ps(v, 1);
        let lo = _mm256_castps256_ps128(v);
        let sum128 = _mm_add_ps(hi, lo);
        let shuf = _mm_movehdup_ps(sum128);
        let sum64 = _mm_add_ps(sum128, shuf);
        let shuf2 = _mm_movehl_ps(sum64, sum64);
        let sum32 = _mm_add_ss(sum64, shuf2);
        _mm_cvtss_f32(sum32)
    }
}

// ---------------------------------------------------------------------------
// NEON (aarch64 — Apple Silicon / ARM)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "aarch64")]
mod neon {
    use std::arch::aarch64::*;

    pub unsafe fn weighted_accumulate_neon(dst: &mut [f32], src: &[f32], weight: f32) {
        let n = dst.len();
        let w = vdupq_n_f32(weight);
        let chunks = n / 4;

        for i in 0..chunks {
            let offset = i * 4;
            let d = vld1q_f32(dst.as_ptr().add(offset));
            let s = vld1q_f32(src.as_ptr().add(offset));
            let result = vfmaq_f32(d, s, w); // d + s * w
            vst1q_f32(dst.as_mut_ptr().add(offset), result);
        }

        for i in (chunks * 4)..n {
            *dst.get_unchecked_mut(i) += src.get_unchecked(i) * weight;
        }
    }

    pub unsafe fn dot_product_neon(a: &[f32], b: &[f32]) -> f32 {
        let n = a.len();
        let chunks = n / 4;
        let mut acc = vdupq_n_f32(0.0);

        for i in 0..chunks {
            let offset = i * 4;
            let va = vld1q_f32(a.as_ptr().add(offset));
            let vb = vld1q_f32(b.as_ptr().add(offset));
            acc = vfmaq_f32(acc, va, vb);
        }

        let mut result = vaddvq_f32(acc);

        for i in (chunks * 4)..n {
            result += a.get_unchecked(i) * b.get_unchecked(i);
        }

        result
    }

    pub unsafe fn norm_sq_neon(v: &[f32]) -> f32 {
        let n = v.len();
        let chunks = n / 4;
        let mut acc = vdupq_n_f32(0.0);

        for i in 0..chunks {
            let offset = i * 4;
            let va = vld1q_f32(v.as_ptr().add(offset));
            acc = vfmaq_f32(acc, va, va);
        }

        let mut result = vaddvq_f32(acc);

        for i in (chunks * 4)..n {
            let x = *v.get_unchecked(i);
            result += x * x;
        }

        result
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_weighted_accumulate() {
        let mut dst = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let src = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        weighted_accumulate(&mut dst, &src, 2.0);
        let expected = vec![2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0];
        for (a, b) in dst.iter().zip(expected.iter()) {
            assert!((a - b).abs() < 1e-6, "{} != {}", a, b);
        }
    }

    #[test]
    fn test_dot_product() {
        let a = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0];
        let b = vec![9.0, 8.0, 7.0, 6.0, 5.0, 4.0, 3.0, 2.0, 1.0];
        let result = dot_product(&a, &b);
        let expected: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        assert!((result - expected).abs() < 1e-4, "{} != {}", result, expected);
    }

    #[test]
    fn test_norm_sq() {
        let v = vec![3.0, 4.0]; // 9 + 16 = 25
        assert!((norm_sq(&v) - 25.0).abs() < 1e-6);
    }

    #[test]
    fn test_simd_matches_scalar() {
        // Test with a size that exercises both SIMD lanes and scalar remainder
        let n = 259; // not divisible by 8 or 4
        let a: Vec<f32> = (0..n).map(|i| (i as f32) * 0.1).collect();
        let b: Vec<f32> = (0..n).map(|i| ((n - i) as f32) * 0.1).collect();

        let dot_simd = dot_product(&a, &b);
        let dot_scalar = scalar::dot_product_scalar(&a, &b);
        assert!(
            (dot_simd - dot_scalar).abs() < 0.1,
            "dot: simd={} scalar={}",
            dot_simd, dot_scalar
        );

        let norm_simd = norm_sq(&a);
        let norm_scalar = scalar::norm_sq_scalar(&a);
        assert!(
            (norm_simd - norm_scalar).abs() < 0.1,
            "norm: simd={} scalar={}",
            norm_simd, norm_scalar
        );

        let mut dst_simd = vec![0.0f32; n];
        let mut dst_scalar = vec![0.0f32; n];
        weighted_accumulate(&mut dst_simd, &a, 2.5);
        scalar::weighted_accumulate_scalar(&mut dst_scalar, &a, 2.5);
        for i in 0..n {
            assert!(
                (dst_simd[i] - dst_scalar[i]).abs() < 1e-4,
                "accumulate[{}]: simd={} scalar={}",
                i, dst_simd[i], dst_scalar[i]
            );
        }
    }

    #[test]
    fn test_empty_vectors() {
        let a: Vec<f32> = vec![];
        let b: Vec<f32> = vec![];
        assert_eq!(dot_product(&a, &b), 0.0);
        assert_eq!(norm_sq(&a), 0.0);
    }
}
