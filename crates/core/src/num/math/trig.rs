// Ported from the Rust `libm` crate 0.2.16 (MIT), origin musl / FreeBSD msun
// (`sin.rs`, `cos.rs`, `sincos.rs`, `tan.rs`, `k_sin.rs`, `k_cos.rs`, `k_tan.rs`,
// `rem_pio2.rs`). Changes: no FP-exception side effects (`force_eval!`), no arch-specific
// paths, plain indexing.

use super::rem_pio2_large::rem_pio2_large;
use super::support::get_high_word;

// origin: FreeBSD /usr/src/lib/msun/src/s_sin.c */
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

// sin(x)
// Return sine function of x.
//
// kernel function:
//      k_sin            ... sine function on [-pi/4,pi/4]
//      k_cos            ... cose function on [-pi/4,pi/4]
//      rem_pio2         ... argument reduction routine
//
// Method.
//      Let S,C and T denote the sin, cos and tan respectively on
//      [-PI/4, +PI/4]. Reduce the argument x to y1+y2 = x-k*pi/2
//      in [-pi/4 , +pi/4], and let n = k mod 4.
//      We have
//
//          n        sin(x)      cos(x)        tan(x)
//     ----------------------------------------------------------
//          0          S           C             T
//          1          C          -S            -1/T
//          2         -S          -C             T
//          3         -C           S            -1/T
//     ----------------------------------------------------------
//
// Special cases:
//      Let trig be any of sin, cos, or tan.
//      trig(+-INF)  is NaN, with signals;
//      trig(NaN)    is that NaN;
//
// Accuracy:
//      TRIG(x) returns trig(x) nearly rounded

/// Sine of `x` (radians).
pub fn sin(x: f64) -> f64 {
    /* High word of x. */
    let ix = (f64::to_bits(x) >> 32) as u32 & 0x7fffffff;

    /* |x| ~< pi/4 */
    if ix <= 0x3fe921fb {
        if ix < 0x3e500000 {
            /* |x| < 2**-26 */
            return x;
        }
        return k_sin(x, 0.0, 0);
    }

    /* sin(Inf or NaN) is NaN */
    if ix >= 0x7ff00000 {
        return x - x;
    }

    /* argument reduction needed */
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => k_sin(y0, y1, 1),
        1 => k_cos(y0, y1),
        2 => -k_sin(y0, y1, 1),
        _ => -k_cos(y0, y1),
    }
}

// origin: FreeBSD /usr/src/lib/msun/src/s_cos.c */
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

/// Cosine of `x` (radians).
pub fn cos(x: f64) -> f64 {
    let ix = (f64::to_bits(x) >> 32) as u32 & 0x7fffffff;

    /* |x| ~< pi/4 */
    if ix <= 0x3fe921fb {
        if ix < 0x3e46a09e {
            /* if x < 2**-27 * sqrt(2) */
            return 1.0;
        }
        return k_cos(x, 0.0);
    }

    /* cos(Inf or NaN) is NaN */
    if ix >= 0x7ff00000 {
        return x - x;
    }

    /* argument reduction needed */
    let (n, y0, y1) = rem_pio2(x);
    match n & 3 {
        0 => k_cos(y0, y1),
        1 => -k_sin(y0, y1, 1),
        2 => -k_cos(y0, y1),
        _ => k_sin(y0, y1, 1),
    }
}

/* origin: FreeBSD /usr/src/lib/msun/src/s_sin.c */
/*
 * ====================================================
 * Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
 *
 * Developed at SunPro, a Sun Microsystems, Inc. business.
 * Permission to use, copy, modify, and distribute this
 * software is freely granted, provided that this notice
 * is preserved.
 * ====================================================
 */

/// `(sin(x), cos(x))`, sharing one argument reduction. Same bits as [`sin`] and [`cos`].
pub fn sin_cos(x: f64) -> (f64, f64) {
    let ix = get_high_word(x) & 0x7fffffff;

    /* |x| ~< pi/4 */
    if ix <= 0x3fe921fb {
        /* if |x| < 2**-27 * sqrt(2) */
        if ix < 0x3e46a09e {
            return (x, 1.0);
        }
        return (k_sin(x, 0.0, 0), k_cos(x, 0.0));
    }

    /* sincos(Inf or NaN) is NaN */
    if ix >= 0x7ff00000 {
        let rv = x - x;
        return (rv, rv);
    }

    /* argument reduction needed */
    let (n, y0, y1) = rem_pio2(x);
    let s = k_sin(y0, y1, 1);
    let c = k_cos(y0, y1);
    match n & 3 {
        0 => (s, c),
        1 => (c, -s),
        2 => (-s, -c),
        _ => (-c, s),
    }
}

// origin: FreeBSD /usr/src/lib/msun/src/s_tan.c */
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

/// Tangent of `x` (radians).
pub fn tan(x: f64) -> f64 {
    let ix = (f64::to_bits(x) >> 32) as u32 & 0x7fffffff;

    /* |x| ~< pi/4 */
    if ix <= 0x3fe921fb {
        if ix < 0x3e400000 {
            /* |x| < 2**-27 */
            return x;
        }
        return k_tan(x, 0.0, 0);
    }

    /* tan(Inf or NaN) is NaN */
    if ix >= 0x7ff00000 {
        return x - x;
    }

    /* argument reduction */
    let (n, y0, y1) = rem_pio2(x);
    k_tan(y0, y1, n & 1)
}

// origin: FreeBSD /usr/src/lib/msun/src/k_sin.c
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

const S1: f64 = -1.66666666666666324348e-01; /* 0xBFC55555, 0x55555549 */
const S2: f64 = 8.33333333332248946124e-03; /* 0x3F811111, 0x1110F8A6 */
const S3: f64 = -1.98412698298579493134e-04; /* 0xBF2A01A0, 0x19C161D5 */
const S4: f64 = 2.75573137070700676789e-06; /* 0x3EC71DE3, 0x57B1FE7D */
const S5: f64 = -2.50507602534068634195e-08; /* 0xBE5AE5E6, 0x8A2B9CEB */
const S6: f64 = 1.58969099521155010221e-10; /* 0x3DE5D93A, 0x5ACFD57C */

// kernel sin function on ~[-pi/4, pi/4] (except on -0), pi/4 ~ 0.7854
// Input x is assumed to be bounded by ~pi/4 in magnitude.
// Input y is the tail of x.
// Input iy indicates whether y is 0. (if iy=0, y assume to be 0).
//
// Algorithm
//      1. Since sin(-x) = -sin(x), we need only to consider positive x.
//      2. Callers must return sin(-0) = -0 without calling here since our
//         odd polynomial is not evaluated in a way that preserves -0.
//         Callers may do the optimization sin(x) ~ x for tiny x.
//      3. sin(x) is approximated by a polynomial of degree 13 on
//         [0,pi/4]
//                               3            13
//              sin(x) ~ x + S1*x + ... + S6*x
//         where
//
//      |sin(x)         2     4     6     8     10     12  |     -58
//      |----- - (1+S1*x +S2*x +S3*x +S4*x +S5*x  +S6*x   )| <= 2
//      |  x                                               |
//
//      4. sin(x+y) = sin(x) + sin'(x')*y
//                  ~ sin(x) + (1-x*x/2)*y
//         For better accuracy, let
//                   3      2      2      2      2
//              r = x *(S2+x *(S3+x *(S4+x *(S5+x *S6))))
//         then                   3    2
//              sin(x) = x + (S1*x + (x *(r-y/2)+y))
fn k_sin(x: f64, y: f64, iy: i32) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = S2 + z * (S3 + z * S4) + z * w * (S5 + z * S6);
    let v = z * x;
    if iy == 0 { x + v * (S1 + z * r) } else { x - ((z * (0.5 * y - v * r) - y) - v * S1) }
}

// origin: FreeBSD /usr/src/lib/msun/src/k_cos.c
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunSoft, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

const C1: f64 = 4.16666666666666019037e-02; /* 0x3FA55555, 0x5555554C */
const C2: f64 = -1.38888888888741095749e-03; /* 0xBF56C16C, 0x16C15177 */
const C3: f64 = 2.48015872894767294178e-05; /* 0x3EFA01A0, 0x19CB1590 */
const C4: f64 = -2.75573143513906633035e-07; /* 0xBE927E4F, 0x809C52AD */
const C5: f64 = 2.08757232129817482790e-09; /* 0x3E21EE9E, 0xBDB4B1C4 */
const C6: f64 = -1.13596475577881948265e-11; /* 0xBDA8FAE9, 0xBE8838D4 */

// kernel cos function on [-pi/4, pi/4], pi/4 ~ 0.785398164
// Input x is assumed to be bounded by ~pi/4 in magnitude.
// Input y is the tail of x.
//
// Algorithm
//      1. Since cos(-x) = cos(x), we need only to consider positive x.
//      2. if x < 2^-27 (hx<0x3e400000 0), return 1 with inexact if x!=0.
//      3. cos(x) is approximated by a polynomial of degree 14 on
//         [0,pi/4]
//                                       4            14
//              cos(x) ~ 1 - x*x/2 + C1*x + ... + C6*x
//         where the remez error is
//
//      |              2     4     6     8     10    12     14 |     -58
//      |cos(x)-(1-.5*x +C1*x +C2*x +C3*x +C4*x +C5*x  +C6*x  )| <= 2
//      |                                                      |
//
//                     4     6     8     10    12     14
//      4. let r = C1*x +C2*x +C3*x +C4*x +C5*x  +C6*x  , then
//             cos(x) ~ 1 - x*x/2 + r
//         since cos(x+y) ~ cos(x) - sin(x)*y
//                        ~ cos(x) - x*y,
//         a correction term is necessary in cos(x) and hence
//              cos(x+y) = 1 - (x*x/2 - (r - x*y))
//         For better accuracy, rearrange to
//              cos(x+y) ~ w + (tmp + (r-x*y))
//         where w = 1 - x*x/2 and tmp is a tiny correction term
//         (1 - x*x/2 == w + tmp exactly in infinite precision).
//         The exactness of w + tmp in infinite precision depends on w
//         and tmp having the same precision as x.  If they have extra
//         precision due to compiler bugs, then the extra precision is
//         only good provided it is retained in all terms of the final
//         expression for cos().  Retention happens in all cases tested
//         under FreeBSD, so don't pessimize things by forcibly clipping
//         any extra precision in w.
fn k_cos(x: f64, y: f64) -> f64 {
    let z = x * x;
    let w = z * z;
    let r = z * (C1 + z * (C2 + z * C3)) + w * w * (C4 + z * (C5 + z * C6));
    let hz = 0.5 * z;
    let w = 1.0 - hz;
    w + (((1.0 - w) - hz) + (z * r - x * y))
}

// origin: FreeBSD /usr/src/lib/msun/src/k_tan.c */
//
// ====================================================
// Copyright 2004 Sun Microsystems, Inc.  All Rights Reserved.
//
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================

// kernel tan function on ~[-pi/4, pi/4] (except on -0), pi/4 ~ 0.7854
// Input x is assumed to be bounded by ~pi/4 in magnitude.
// Input y is the tail of x.
// Input odd indicates whether tan (if odd = 0) or -1/tan (if odd = 1) is returned.
//
// Algorithm
//      1. Since tan(-x) = -tan(x), we need only to consider positive x.
//      2. Callers must return tan(-0) = -0 without calling here since our
//         odd polynomial is not evaluated in a way that preserves -0.
//         Callers may do the optimization tan(x) ~ x for tiny x.
//      3. tan(x) is approximated by a odd polynomial of degree 27 on
//         [0,0.67434]
//                               3             27
//              tan(x) ~ x + T1*x + ... + T13*x
//         where
//
//              |tan(x)         2     4            26   |     -59.2
//              |----- - (1+T1*x +T2*x +.... +T13*x    )| <= 2
//              |  x                                    |
//
//         Note: tan(x+y) = tan(x) + tan'(x)*y
//                        ~ tan(x) + (1+x*x)*y
//         Therefore, for better accuracy in computing tan(x+y), let
//                   3      2      2       2       2
//              r = x *(T2+x *(T3+x *(...+x *(T12+x *T13))))
//         then
//                                  3    2
//              tan(x+y) = x + (T1*x + (x *(r+y)+y))
//
//      4. For x in [0.67434,pi/4],  let y = pi/4 - x, then
//              tan(x) = tan(pi/4-y) = (1-tan(y))/(1+tan(y))
//                     = 1 - 2*(tan(y) - (tan(y)^2)/(1+tan(y)))
const T: [f64; 13] = [
    3.33333333333334091986e-01,  /* 3FD55555, 55555563 */
    1.33333333333201242699e-01,  /* 3FC11111, 1110FE7A */
    5.39682539762260521377e-02,  /* 3FABA1BA, 1BB341FE */
    2.18694882948595424599e-02,  /* 3F9664F4, 8406D637 */
    8.86323982359930005737e-03,  /* 3F8226E3, E96E8493 */
    3.59207910759131235356e-03,  /* 3F6D6D22, C9560328 */
    1.45620945432529025516e-03,  /* 3F57DBC8, FEE08315 */
    5.88041240820264096874e-04,  /* 3F4344D8, F2F26501 */
    2.46463134818469906812e-04,  /* 3F3026F7, 1A8D1068 */
    7.81794442939557092300e-05,  /* 3F147E88, A03792A6 */
    7.14072491382608190305e-05,  /* 3F12B80F, 32F0A7E9 */
    -1.85586374855275456654e-05, /* BEF375CB, DB605373 */
    2.59073051863633712884e-05,  /* 3EFB2A70, 74BF7AD4 */
];
const PIO4: f64 = 7.85398163397448278999e-01; /* 3FE921FB, 54442D18 */
const PIO4_LO: f64 = 3.06161699786838301793e-17; /* 3C81A626, 33145C07 */

fn k_tan(mut x: f64, mut y: f64, odd: i32) -> f64 {
    let hx = (f64::to_bits(x) >> 32) as u32;
    let big = (hx & 0x7fffffff) >= 0x3FE59428; /* |x| >= 0.6744 */
    if big {
        let sign = hx >> 31;
        if sign != 0 {
            x = -x;
            y = -y;
        }
        x = (PIO4 - x) + (PIO4_LO - y);
        y = 0.0;
    }
    let z = x * x;
    let w = z * z;

    /*
     * Break x^5*(T[1]+x^2*T[2]+...) into
     * x^5(T[1]+x^4*T[3]+...+x^20*T[11]) +
     * x^5(x^2*(T[2]+x^4*T[4]+...+x^22*[T12]))
     */
    let r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))));
    let v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))));
    let s = z * x;
    let r = y + z * (s * (r + v) + y) + s * T[0];
    let w = x + r;
    if big {
        let sign = hx >> 31;
        let s = 1.0 - 2.0 * odd as f64;
        let v = s - 2.0 * (x + (r - w * w / (w + s)));
        return if sign != 0 { -v } else { v };
    }
    if odd == 0 {
        return w;
    }
    /* -1.0/(x+r) has up to 2ulp error, so compute it accurately */
    let w0 = zero_low_word(w);
    let v = r - (w0 - x); /* w0+v = r+x */
    let a = -1.0 / w;
    let a0 = zero_low_word(a);
    a0 + a * (1.0 + a0 * w0 + a0 * v)
}

fn zero_low_word(x: f64) -> f64 {
    f64::from_bits(f64::to_bits(x) & 0xFFFF_FFFF_0000_0000)
}

// origin: FreeBSD /usr/src/lib/msun/src/e_rem_pio2.c
//
// ====================================================
// Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.
//
// Developed at SunPro, a Sun Microsystems, Inc. business.
// Permission to use, copy, modify, and distribute this
// software is freely granted, provided that this notice
// is preserved.
// ====================================================
//
// Optimized by Bruce D. Evans. */
// #if FLT_EVAL_METHOD==0 || FLT_EVAL_METHOD==1
// #define EPS DBL_EPSILON
const EPS: f64 = 2.2204460492503131e-16;
// #elif FLT_EVAL_METHOD==2
// #define EPS LDBL_EPSILON
// #endif

const TO_INT: f64 = 1.5 / EPS;
// 53 bits of 2/pi
const INV_PIO2: f64 = 6.36619772367581382433e-01; /* 0x3FE45F30, 0x6DC9C883 */
// first 33 bits of pi/2
const PIO2_1: f64 = 1.57079632673412561417e+00; /* 0x3FF921FB, 0x54400000 */
// pi/2 - PIO2_1
const PIO2_1T: f64 = 6.07710050650619224932e-11; /* 0x3DD0B461, 0x1A626331 */
// second 33 bits of pi/2
const PIO2_2: f64 = 6.07710050630396597660e-11; /* 0x3DD0B461, 0x1A600000 */
// pi/2 - (PIO2_1+PIO2_2)
const PIO2_2T: f64 = 2.02226624879595063154e-21; /* 0x3BA3198A, 0x2E037073 */
// third 33 bits of pi/2
const PIO2_3: f64 = 2.02226624871116645580e-21; /* 0x3BA3198A, 0x2E000000 */
// pi/2 - (PIO2_1+PIO2_2+PIO2_3)
const PIO2_3T: f64 = 8.47842766036889956997e-32; /* 0x397B839A, 0x252049C1 */

/// Medium-size case of [`rem_pio2`]: `|x| ~< 2^20 * pi/2`.
fn rem_pio2_medium(x: f64, ix: u32) -> (i32, f64, f64) {
    /* rint(x/(pi/2)), Assume round-to-nearest. */
    let tmp = x * INV_PIO2 + TO_INT;
    let f_n = tmp - TO_INT;
    let n = f_n as i32;
    let mut r = x - f_n * PIO2_1;
    let mut w = f_n * PIO2_1T; /* 1st round, good to 85 bits */
    let mut y0 = r - w;
    let ui = f64::to_bits(y0);
    let ey = (ui >> 52) as i32 & 0x7ff;
    let ex = (ix >> 20) as i32;
    if ex - ey > 16 {
        /* 2nd round, good to 118 bits */
        let t = r;
        w = f_n * PIO2_2;
        r = t - w;
        w = f_n * PIO2_2T - ((t - r) - w);
        y0 = r - w;
        let ey = (f64::to_bits(y0) >> 52) as i32 & 0x7ff;
        if ex - ey > 49 {
            /* 3rd round, good to 151 bits, covers all cases */
            let t = r;
            w = f_n * PIO2_3;
            r = t - w;
            w = f_n * PIO2_3T - ((t - r) - w);
            y0 = r - w;
        }
    }
    let y1 = (r - y0) - w;
    (n, y0, y1)
}

// return the remainder of x rem pi/2 in y[0]+y[1]
// use rem_pio2_large() for large x
//
// caller must handle the case when reduction is not needed: |x| ~<= pi/4 */
fn rem_pio2(x: f64) -> (i32, f64, f64) {
    let x1p24 = f64::from_bits(0x4170000000000000);

    let sign = (f64::to_bits(x) >> 63) as i32;
    let ix = (f64::to_bits(x) >> 32) as u32 & 0x7fffffff;

    if ix <= 0x400f6a7a {
        /* |x| ~<= 5pi/4 */
        if (ix & 0xfffff) == 0x921fb {
            /* |x| ~= pi/2 or 2pi/2 */
            return rem_pio2_medium(x, ix); /* cancellation -- use medium case */
        }
        if ix <= 0x4002d97c {
            /* |x| ~<= 3pi/4 */
            if sign == 0 {
                let z = x - PIO2_1; /* one round good to 85 bits */
                let y0 = z - PIO2_1T;
                let y1 = (z - y0) - PIO2_1T;
                return (1, y0, y1);
            }
            let z = x + PIO2_1;
            let y0 = z + PIO2_1T;
            let y1 = (z - y0) + PIO2_1T;
            return (-1, y0, y1);
        } else if sign == 0 {
            let z = x - 2.0 * PIO2_1;
            let y0 = z - 2.0 * PIO2_1T;
            let y1 = (z - y0) - 2.0 * PIO2_1T;
            return (2, y0, y1);
        }
        let z = x + 2.0 * PIO2_1;
        let y0 = z + 2.0 * PIO2_1T;
        let y1 = (z - y0) + 2.0 * PIO2_1T;
        return (-2, y0, y1);
    }
    if ix <= 0x401c463b {
        /* |x| ~<= 9pi/4 */
        if ix <= 0x4015fdbc {
            /* |x| ~<= 7pi/4 */
            if ix == 0x4012d97c {
                /* |x| ~= 3pi/2 */
                return rem_pio2_medium(x, ix);
            }
            if sign == 0 {
                let z = x - 3.0 * PIO2_1;
                let y0 = z - 3.0 * PIO2_1T;
                let y1 = (z - y0) - 3.0 * PIO2_1T;
                return (3, y0, y1);
            }
            let z = x + 3.0 * PIO2_1;
            let y0 = z + 3.0 * PIO2_1T;
            let y1 = (z - y0) + 3.0 * PIO2_1T;
            return (-3, y0, y1);
        }
        if ix == 0x401921fb {
            /* |x| ~= 4pi/2 */
            return rem_pio2_medium(x, ix);
        }
        if sign == 0 {
            let z = x - 4.0 * PIO2_1;
            let y0 = z - 4.0 * PIO2_1T;
            let y1 = (z - y0) - 4.0 * PIO2_1T;
            return (4, y0, y1);
        }
        let z = x + 4.0 * PIO2_1;
        let y0 = z + 4.0 * PIO2_1T;
        let y1 = (z - y0) + 4.0 * PIO2_1T;
        return (-4, y0, y1);
    }
    if ix < 0x413921fb {
        /* |x| ~< 2^20*(pi/2), medium size */
        return rem_pio2_medium(x, ix);
    }
    /*
     * all other (large) arguments
     */
    if ix >= 0x7ff00000 {
        /* x is inf or NaN */
        let y0 = x - x;
        let y1 = y0;
        return (0, y0, y1);
    }
    /* set z = scalbn(|x|,-ilogb(x)+23) */
    let mut ui = f64::to_bits(x);
    ui &= (!1) >> 12;
    ui |= (0x3ff + 23) << 52;
    let mut z = f64::from_bits(ui);
    let mut tx = [0.0; 3];
    for t in tx.iter_mut().take(2) {
        *t = z as i32 as f64;
        z = (z - *t) * x1p24;
    }
    tx[2] = z;
    /* skip zero terms, first term is non-zero */
    let mut i = 2;
    while i != 0 && tx[i] == 0.0 {
        i -= 1;
    }
    let mut ty = [0.0; 2];
    let n = rem_pio2_large(&tx[..=i], &mut ty, ((ix as i32) >> 20) - (0x3ff + 23));
    if sign != 0 {
        return (-n, -ty[0], -ty[1]);
    }
    (n, ty[0], ty[1])
}

#[cfg(test)]
mod tests {
    use super::rem_pio2;

    // From libm's own tests (`rem_pio2.rs`).
    #[test]
    fn rem_pio2_near_pi() {
        assert_eq!(rem_pio2(3.141592025756836), (2, -6.278329573009626e-7, -2.1125998133974653e-23));
        assert_eq!(rem_pio2(3.141592033207416), (2, -6.20382377148128e-7, -2.1125998133974653e-23));
        assert_eq!(rem_pio2(3.141592144966125), (2, -5.086236681942706e-7, -2.1125998133974653e-23));
        assert_eq!(rem_pio2(3.141592979431152), (2, 3.2584135866119817e-7, -2.1125998133974653e-23));
    }

    #[test]
    fn rem_pio2_no_overflow() {
        let _ = rem_pio2(-3054214.5490637687);
        let _ = rem_pio2(917340800458.2274);
    }
}
