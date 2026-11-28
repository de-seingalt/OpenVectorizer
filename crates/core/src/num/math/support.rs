// Ported from the Rust `libm` crate 0.2.16 (MIT), origin musl / FreeBSD msun.
//
// Word access helpers (libm `src/math/mod.rs`) and `scalbn` (libm `src/math/generic/scalbn.rs`,
// specialised here to `f64`). Copyright (c) 2018 Jorge Aparicio and the libm contributors.

/// High 32 bits of the IEEE representation of `x`.
#[inline]
pub(super) fn get_high_word(x: f64) -> u32 {
    (x.to_bits() >> 32) as u32
}

/// Low 32 bits of the IEEE representation of `x`.
#[inline]
pub(super) fn get_low_word(x: f64) -> u32 {
    x.to_bits() as u32
}

/// `f` with its high 32 bits replaced by `hi`.
#[inline]
pub(super) fn with_set_high_word(f: f64, hi: u32) -> f64 {
    let mut tmp = f.to_bits();
    tmp &= 0x0000_0000_ffff_ffff;
    tmp |= u64::from(hi) << 32;
    f64::from_bits(tmp)
}

/// `f` with its low 32 bits replaced by `lo`.
#[inline]
pub(super) fn with_set_low_word(f: f64, lo: u32) -> f64 {
    let mut tmp = f.to_bits();
    tmp &= 0xffff_ffff_0000_0000;
    tmp |= u64::from(lo);
    f64::from_bits(tmp)
}

/// `x * 2^n`, exact unless the result overflows or underflows (C `scalbn`).
pub(super) fn scalbn(mut x: f64, mut n: i32) -> f64 {
    const EXP_MAX: i32 = 1023;
    const EXP_MIN: i32 = -1022;
    const ADD: i32 = -EXP_MIN - 53;
    // 2^1023, the largest power of two.
    let f_exp_max = f64::from_bits(0x7fe0_0000_0000_0000);
    // 2^-1022 * 2^53: prescaling by it never makes `x` subnormal (avoids double rounding).
    let mul = f64::from_bits(0x0010_0000_0000_0000) * f64::from_bits(0x4340_0000_0000_0000);

    if n > EXP_MAX {
        x *= f_exp_max;
        n -= EXP_MAX;
        if n > EXP_MAX {
            x *= f_exp_max;
            n -= EXP_MAX;
            if n > EXP_MAX {
                n = EXP_MAX;
            }
        }
    } else if n < EXP_MIN {
        x *= mul;
        n += ADD;
        if n < EXP_MIN {
            x *= mul;
            n += ADD;
            if n < EXP_MIN {
                n = EXP_MIN;
            }
        }
    }
    let scale = f64::from_bits(((1023 + n) as u64) << 52);
    x * scale
}
