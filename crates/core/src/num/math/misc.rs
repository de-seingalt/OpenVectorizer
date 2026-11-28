// Ported from the Rust `libm` crate (MIT), origin musl / FreeBSD msun: `cbrt` from libm 0.2.11
// (the FreeBSD `s_cbrt.c` port; libm 0.2.16 replaced it with a CORE-MATH port that needs `fma`),
// `hypot` from libm 0.2.16 (musl `hypot.c`). Changes: `sqrt` from `std` (exact IEEE operation).

/* origin: FreeBSD /usr/src/lib/msun/src/s_cbrt.c */
/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 *
 * Optimized by Bruce D. Evans.
 */
/* cbrt(x)
 * Return cube root of x
 */

const B1: u32 = 715094163; /* B1 = (1023-1023/3-0.03306235651)*2**20 */
const B2: u32 = 696219795; /* B2 = (1023-1023/3-54/3-0.03306235651)*2**20 */

/* |1/cbrt(x) - p(x)| < 2**-23.5 (~[-7.93e-8, 7.929e-8]). */
const P0: f64 = 1.87595182427177009643; /* 0x3ffe03e6, 0x0f61e692 */
const P1: f64 = -1.88497979543377169875; /* 0xbffe28e0, 0x92f02420 */
const P2: f64 = 1.621429720105354466140; /* 0x3ff9f160, 0x4a49d6c2 */
const P3: f64 = -0.758397934778766047437; /* 0xbfe844cb, 0xbee751d9 */
const P4: f64 = 0.145996192886612446982; /* 0x3fc2b000, 0xd4e4edd7 */

/// Cube root of `x` (error < 0.667 ulp); `cbrt(-x) = -cbrt(x)`.
pub fn cbrt(x: f64) -> f64 {
    let x1p54 = f64::from_bits(0x4350000000000000); // 0x1p54 === 2 ^ 54

    let mut ui: u64 = x.to_bits();
    let mut hx: u32 = (ui >> 32) as u32 & 0x7fffffff;

    if hx >= 0x7ff00000 {
        /* cbrt(NaN,INF) is itself */
        return x + x;
    }

    /*
     * Rough cbrt to 5 bits:
     *    cbrt(2**e*(1+m) ~= 2**(e/3)*(1+(e%3+m)/3)
     * where e is integral and >= 0, m is real and in [0, 1), and "/" and
     * "%" are integer division and modulus with rounding towards minus
     * infinity.  The RHS is always >= the LHS and has a maximum relative
     * error of about 1 in 16.  Adding a bias of -0.03306235651 to the
     * (e%3+m)/3 term reduces the error to about 1 in 32. With the IEEE
     * floating point representation, for finite positive normal values,
     * ordinary integer divison of the value in bits magically gives
     * almost exactly the RHS of the above provided we first subtract the
     * exponent bias (1023 for doubles) and later add it back.  We do the
     * subtraction virtually to keep e >= 0 so that ordinary integer
     * division rounds towards minus infinity; this is also efficient.
     */
    if hx < 0x00100000 {
        /* zero or subnormal? */
        ui = (x * x1p54).to_bits();
        hx = (ui >> 32) as u32 & 0x7fffffff;
        if hx == 0 {
            return x; /* cbrt(0) is itself */
        }
        hx = hx / 3 + B2;
    } else {
        hx = hx / 3 + B1;
    }
    ui &= 1 << 63;
    ui |= (hx as u64) << 32;
    let mut t = f64::from_bits(ui);

    /*
     * New cbrt to 23 bits:
     *    cbrt(x) = t*cbrt(x/t**3) ~= t*P(t**3/x)
     * where P(r) is a polynomial of degree 4 that approximates 1/cbrt(r)
     * to within 2**-23.5 when |r - 1| < 1/10.  The rough approximation
     * has produced t such than |t/cbrt(x) - 1| ~< 1/32, and cubing this
     * gives us bounds for r = t**3/x.
     *
     * Try to optimize for parallel evaluation as in __tanf.c.
     */
    let mut r = (t * t) * (t / x);
    t *= (P0 + r * (P1 + r * P2)) + ((r * r) * r) * (P3 + r * P4);

    /*
     * Round t away from zero to 23 bits (sloppily except for ensuring that
     * the result is larger in magnitude than cbrt(x) but not much more than
     * 2 23-bit ulps larger).  With rounding towards zero, the error bound
     * would be ~5/6 instead of ~4/6.  With a maximum error of 2 23-bit ulps
     * in the rounded t, the infinite-precision error in the Newton
     * approximation barely affects third digit in the final error
     * 0.667; the error in the rounded t can be up to about 3 23-bit ulps
     * before the final error is larger than 0.667 ulps.
     */
    ui = t.to_bits();
    ui = (ui + 0x80000000) & 0xffffffffc0000000;
    t = f64::from_bits(ui);

    /* one step Newton iteration to 53 bits with error < 0.667 ulps */
    let s = t * t; /* t*t is exact */
    r = x / s; /* error <= 0.5 ulps; |r| < |t| */
    let w = t + t; /* t+t is exact */
    r = (r - t) / (w + r); /* r-t is exact; w+r ~= 3*t */
    t + t * r /* error <= 0.5 + 0.5/3 + epsilon */
}

// origin: musl src/math/hypot.c (MIT, Copyright © 2005-2020 Rich Felker, et al.)

const SPLIT: f64 = 134217728. + 1.; // 0x1p27 + 1 === (2 ^ 27) + 1

fn sq(x: f64) -> (f64, f64) {
    let xc = x * SPLIT;
    let xh = x - xc + xc;
    let xl = x - xh;
    let hi = x * x;
    let lo = xh * xh - hi + 2. * xh * xl + xl * xl;
    (hi, lo)
}

/// `sqrt(x*x + y*y)` without undue overflow or underflow; `hypot(±inf, NaN) = +inf`.
pub fn hypot(mut x: f64, mut y: f64) -> f64 {
    let x1p700 = f64::from_bits(0x6bb0000000000000); // 0x1p700 === 2 ^ 700
    let x1p_700 = f64::from_bits(0x1430000000000000); // 0x1p-700 === 2 ^ -700

    let mut uxi = x.to_bits();
    let mut uyi = y.to_bits();

    /* arrange |x| >= |y| */
    uxi &= -1i64 as u64 >> 1;
    uyi &= -1i64 as u64 >> 1;
    if uxi < uyi {
        std::mem::swap(&mut uxi, &mut uyi);
    }

    /* special cases */
    let ex = (uxi >> 52) as i64;
    let ey = (uyi >> 52) as i64;
    x = f64::from_bits(uxi);
    y = f64::from_bits(uyi);
    /* note: hypot(inf,nan) == inf */
    if ey == 0x7ff {
        return y;
    }
    if ex == 0x7ff || uyi == 0 {
        return x;
    }
    /* note: hypot(x,y) ~= x + y*y/x/2 with inexact for small y/x */
    /* 64 difference is enough for ld80 double_t */
    if ex - ey > 64 {
        return x + y;
    }

    /* precise sqrt argument in nearest rounding mode without overflow */
    /* xh*xh must not overflow and xl*xl must not underflow in sq */
    let mut z = 1.;
    if ex > 0x3ff + 510 {
        z = x1p700;
        x *= x1p_700;
        y *= x1p_700;
    } else if ey < 0x3ff - 450 {
        z = x1p_700;
        x *= x1p700;
        y *= x1p700;
    }
    let (hx, lx) = sq(x);
    let (hy, ly) = sq(y);
    z * (ly + lx + hy + hx).sqrt()
}
