//! Shared production primitives with the pinned speech arithmetic order.
//!
//! Codec dot products retain their separate reduction order in `codec`.
#![expect(
    clippy::redundant_pub_crate,
    reason = "Numerical helpers are shared only within this private speech crate module."
)]

use fearless_simd::{f32x4, prelude::*};

#[inline]
pub(super) fn usize_to_f32(n: usize) -> f32 {
    u16::try_from(n).map_or_else(
        |_| {
            let hi = u16::try_from(n >> 16).unwrap_or(u16::MAX);
            let lo = u16::try_from(n & 0xFFFF).unwrap_or(0);
            f32::from(hi).mul_add(65_536.0_f32, f32::from(lo))
        },
        f32::from,
    )
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
pub(super) fn f32_dot_simd<S: Simd>(simd: S, a: &[f32], b: &[f32]) -> f32 {
    let (a_chunks, a_tail) = a.as_chunks::<4>();
    let (b_chunks, b_tail) = b.as_chunks::<4>();
    let mut acc0 = f32x4::splat(simd, 0.0_f32);
    let mut acc1 = f32x4::splat(simd, 0.0_f32);
    let (a_pairs, a_rem) = a_chunks.as_chunks::<2>();
    let (b_pairs, b_rem) = b_chunks.as_chunks::<2>();
    for (ap, bp) in a_pairs.iter().zip(b_pairs.iter()) {
        let a0 = f32x4::from_slice(simd, &ap[0]);
        let b0 = f32x4::from_slice(simd, &bp[0]);
        let a1 = f32x4::from_slice(simd, &ap[1]);
        let b1 = f32x4::from_slice(simd, &bp[1]);
        acc0 = a0.mul_add(b0, acc0);
        acc1 = a1.mul_add(b1, acc1);
    }
    for (ac, bc) in a_rem.iter().zip(b_rem.iter()) {
        let av = f32x4::from_slice(simd, ac);
        let bv = f32x4::from_slice(simd, bc);
        acc0 = av.mul_add(bv, acc0);
    }
    let sum_v = acc0 + acc1;
    let mut lanes = [0.0_f32; 4];
    sum_v.store_slice(&mut lanes);
    let mut total = (lanes[0] + lanes[1]) + (lanes[2] + lanes[3]);
    for (&av, &bv) in a_tail.iter().zip(b_tail.iter()) {
        total = av.mul_add(bv, total);
    }
    total
}

#[expect(
    clippy::inline_always,
    reason = "fearless_simd dispatch! requires inline(always) to inherit target_feature attributes."
)]
#[inline(always)]
pub(super) fn axpy_simd<S: Simd>(simd: S, scale: f32, x: &[f32], dst: &mut [f32]) {
    let w_v = f32x4::splat(simd, scale);
    let (x_chunks, x_tail) = x.as_chunks::<4>();
    let (dst_chunks, dst_tail) = dst.as_chunks_mut::<4>();
    for (xc, dc) in x_chunks.iter().zip(dst_chunks.iter_mut()) {
        let xv = f32x4::from_slice(simd, xc);
        let dv = f32x4::from_slice(simd, dc);
        w_v.mul_add(xv, dv).store_slice(dc);
    }
    for (&xi, di) in x_tail.iter().zip(dst_tail.iter_mut()) {
        *di = scale.mul_add(xi, *di);
    }
}
