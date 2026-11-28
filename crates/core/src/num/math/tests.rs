//! Accuracy against `std` on dense deterministic samples, special values, and a bit-exact
//! fingerprint (the same test run on wasm32 proves cross-target determinism).

use std::f64::consts::{FRAC_PI_2, PI};

use super::{acos, asin, atan, atan2, cbrt, cos, exp, hypot, ln, ln_1p, pow, sin, sin_cos, tan};

/// `SplitMix64`: tiny deterministic generator, local to the tests.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[lo, hi)`.
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 * (1.0 / (1u64 << 53) as f64))
    }

    /// Random significand with an exponent in `[emin, emax]` (unbiased; -1023 = subnormal),
    /// random sign when `signed`.
    fn log_uniform(&mut self, emin: i32, emax: i32, signed: bool) -> f64 {
        let span = (emax - emin + 1) as u64;
        let e = emin + (self.next() % span) as i32;
        let mant = self.next() & ((1u64 << 52) - 1);
        let sign = if signed { self.next() & (1u64 << 63) } else { 0 };
        f64::from_bits(sign | (((e + 1023) as u64) << 52) | mant)
    }

    fn subnormal(&mut self) -> f64 {
        self.log_uniform(-1023, -1023, true)
    }

    /// `x` moved by a random number of ulps in `[-d, d]`.
    fn jitter(&mut self, x: f64, d: u64) -> f64 {
        let k = (self.next() % (2 * d + 1)) as i64 - d as i64;
        f64::from_bits((x.to_bits() as i64 + k) as u64)
    }
}

/// Maps the bits of `x` to an integer that is monotonic in `x` (`-0` and `+0` both map to 0).
fn ordered(x: f64) -> i128 {
    let b = x.to_bits() as i64;
    i128::from(if b < 0 { i64::MIN - b } else { b })
}

/// Distance in ulps; 0 if both are NaN, `u64::MAX` if only one is.
fn ulps(a: f64, b: f64) -> u64 {
    match (a.is_nan(), b.is_nan()) {
        (true, true) => 0,
        (false, false) => u64::try_from((ordered(a) - ordered(b)).unsigned_abs()).unwrap_or(u64::MAX),
        _ => u64::MAX,
    }
}

fn same_bits(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan())
}

/// Largest ulp error of `ours` vs `reference` over `inputs`; panics above `bound`.
fn check1(name: &str, ours: fn(f64) -> f64, reference: fn(f64) -> f64, inputs: &[f64], bound: u64) -> u64 {
    let mut worst = (0, 0.0);
    for &x in inputs {
        let e = ulps(ours(x), reference(x));
        if e > worst.0 {
            worst = (e, x);
        }
    }
    eprintln!("{name}: max {} ulp over {} points (at x = {:e})", worst.0, inputs.len(), worst.1);
    assert!(
        worst.0 <= bound,
        "{name}({:e}) = {:e}, std gives {:e}: {} ulp",
        worst.1,
        ours(worst.1),
        reference(worst.1),
        worst.0
    );
    worst.0
}

fn check2(
    name: &str,
    ours: fn(f64, f64) -> f64,
    reference: fn(f64, f64) -> f64,
    inputs: &[(f64, f64)],
    bound: u64,
) -> u64 {
    let mut worst = (0, (0.0, 0.0));
    for &(a, b) in inputs {
        let e = ulps(ours(a, b), reference(a, b));
        if e > worst.0 {
            worst = (e, (a, b));
        }
    }
    let (a, b) = worst.1;
    eprintln!("{name}: max {} ulp over {} points (at {a:e}, {b:e})", worst.0, inputs.len());
    assert!(
        worst.0 <= bound,
        "{name}({a:e}, {b:e}) = {:e}, std gives {:e}: {} ulp",
        ours(a, b),
        reference(a, b),
        worst.0
    );
    worst.0
}

/// `n` points drawn by a generator.
type Part<'a, T> = (usize, &'a dyn Fn(&mut Rng) -> T);

fn sample<T>(seed: u64, parts: &[Part<T>]) -> Vec<T> {
    let mut rng = Rng(seed);
    let mut v = Vec::new();
    for (n, make) in parts {
        v.extend((0..*n).map(|_| make(&mut rng)));
    }
    v
}

/// Arguments for sin/cos/tan: small, tiny, huge (Payne-Hanek path), near multiples of pi/2.
fn trig_inputs() -> Vec<f64> {
    sample(
        1,
        &[
            (30_000, &|r| r.uniform(-10.0, 10.0)),
            (10_000, &|r| r.uniform(-1e6, 1e6)),
            (30_000, &|r| {
                let k = (r.next() % 2001) as f64 - 1000.0;
                r.jitter(k * FRAC_PI_2, 64)
            }),
            (20_000, &|r| r.log_uniform(0, 1023, true)),
            (15_000, &|r| r.log_uniform(-60, -1, true)),
            (2_000, &Rng::subnormal),
        ],
    )
}

#[test]
fn exp_matches_std() {
    let xs = sample(
        2,
        &[
            (40_000, &|r| r.uniform(-746.0, 710.0)),
            (20_000, &|r| r.uniform(-1.0, 1.0)),
            (20_000, &|r| r.log_uniform(-60, 0, true)),
            (10_000, &|r| r.uniform(700.0, 709.79)),
            (10_000, &|r| r.uniform(-745.2, -708.0)),
            (1_000, &Rng::subnormal),
        ],
    );
    check1("exp", exp, f64::exp, &xs, 1);
}

#[test]
fn ln_matches_std() {
    let xs = sample(
        3,
        &[
            (40_000, &|r| r.log_uniform(-1022, 1023, false)),
            (20_000, &|r| 1.0 + r.uniform(-1.0 / 1024.0, 1.0 / 1024.0)),
            (10_000, &|r| r.jitter(1.0, 1000)),
            (20_000, &|r| r.uniform(0.0, 10.0)),
            (10_000, &|r| r.subnormal().abs()),
        ],
    );
    check1("ln", ln, f64::ln, &xs, 1);
}

#[test]
fn ln_1p_matches_std() {
    let xs = sample(
        4,
        &[
            (30_000, &|r| r.uniform(-1.0, 1.0)),
            (30_000, &|r| r.log_uniform(-60, -1, true)),
            (20_000, &|r| r.log_uniform(0, 1023, false)),
            (10_000, &|r| -1.0 + r.log_uniform(-50, -1, false)),
            (10_000, &Rng::subnormal),
        ],
    );
    check1("ln_1p", ln_1p, f64::ln_1p, &xs, 1);
}

#[test]
fn sin_cos_tan_match_std() {
    let xs = trig_inputs();
    check1("sin", sin, f64::sin, &xs, 1);
    check1("cos", cos, f64::cos, &xs, 1);
    check1("tan", tan, f64::tan, &xs, 1);
    for &x in &xs {
        let (s, c) = sin_cos(x);
        assert!(same_bits(s, sin(x)) && same_bits(c, cos(x)), "sin_cos({x:e}) != (sin, cos)");
    }
}

#[test]
fn atan_matches_std() {
    let xs = sample(
        5,
        &[
            (40_000, &|r| r.log_uniform(-1022, 1023, true)),
            (50_000, &|r| r.uniform(-3.0, 3.0)),
            (10_000, &|r| r.log_uniform(-60, 0, true)),
            (2_000, &Rng::subnormal),
        ],
    );
    check1("atan", atan, f64::atan, &xs, 1);
}

#[test]
fn atan2_matches_std() {
    let ps = sample(
        6,
        &[
            (50_000, &|r| (r.log_uniform(-30, 30, true), r.log_uniform(-30, 30, true))),
            (40_000, &|r| (r.uniform(-1.0, 1.0), r.uniform(-1.0, 1.0))),
            (10_000, &|r| (r.log_uniform(-1022, 1023, true), r.log_uniform(-1022, 1023, true))),
            (2_000, &|r| (r.subnormal(), r.subnormal())),
        ],
    );
    check2("atan2", atan2, f64::atan2, &ps, 1);
}

#[test]
fn asin_acos_match_std() {
    let xs = sample(
        7,
        &[
            (60_000, &|r| r.uniform(-1.0, 1.0)),
            (20_000, &|r| {
                let d = r.log_uniform(-53, -1, false);
                if r.next() & 1 == 0 { 1.0 - d } else { d - 1.0 }
            }),
            (20_000, &|r| r.log_uniform(-60, -1, true)),
            (2_000, &Rng::subnormal),
        ],
    );
    check1("asin", asin, f64::asin, &xs, 1);
    check1("acos", acos, f64::acos, &xs, 1);
}

#[test]
fn cbrt_matches_std() {
    let xs = sample(
        8,
        &[
            (60_000, &|r| r.log_uniform(-1022, 1023, true)),
            (30_000, &|r| r.uniform(-10.0, 10.0)),
            (10_000, &Rng::subnormal),
        ],
    );
    check1("cbrt", cbrt, f64::cbrt, &xs, 1);
}

#[test]
fn pow_matches_std() {
    let ps = sample(
        9,
        &[
            (40_000, &|r| (r.log_uniform(-20, 20, false), r.uniform(-40.0, 40.0))),
            (20_000, &|r| {
                let d = r.log_uniform(-40, -1, true);
                (1.0 + d, r.uniform(-1e6, 1e6))
            }),
            (20_000, &|r| (-r.log_uniform(-20, 20, false), (r.next() % 101) as f64 - 50.0)),
            (20_000, &|r| (r.uniform(0.0, 4.0), r.uniform(-800.0, 800.0))),
            (5_000, &|r| (r.subnormal().abs(), r.uniform(-1.0, 1.0))),
        ],
    );
    check2("pow", pow, f64::powf, &ps, 1);
    // pow(integer, integer) is exact when the result is representable.
    for b in 0..=20u32 {
        for e in 0..=15u32 {
            let Some(exact) = u64::from(b).checked_pow(e).filter(|&v| v <= 1 << 53) else {
                continue;
            };
            let exact = exact as f64;
            assert!(same_bits(pow(f64::from(b), f64::from(e)), exact), "pow({b}, {e})");
        }
    }
}

#[test]
fn hypot_matches_std() {
    let ps = sample(
        10,
        &[
            (60_000, &|r| (r.log_uniform(-1022, 1023, true), r.log_uniform(-1022, 1023, true))),
            (30_000, &|r| (r.uniform(-10.0, 10.0), r.uniform(-10.0, 10.0))),
            (10_000, &|r| {
                let x = r.log_uniform(-40, 40, true);
                (x, r.jitter(x, 1 << 40))
            }),
            (2_000, &|r| (r.subnormal(), r.subnormal())),
        ],
    );
    check2("hypot", hypot, f64::hypot, &ps, 1);
}

const SPECIAL: [f64; 18] = [
    0.0,
    -0.0,
    f64::INFINITY,
    f64::NEG_INFINITY,
    f64::NAN,
    1.0,
    -1.0,
    0.5,
    -0.5,
    2.0,
    -2.0,
    3.0,
    -3.0,
    f64::MIN_POSITIVE,
    -f64::MIN_POSITIVE,
    5e-324,
    f64::MAX,
    f64::MIN,
];

/// On special inputs, results are NaN exactly when `std`'s are, zeros and infinities carry the
/// same sign, and finite values are within 1 ulp.
fn special_like_std(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() || a == 0.0 || b == 0.0 || a.is_infinite() || b.is_infinite() {
        same_bits(a, b)
    } else {
        ulps(a, b) <= 1
    }
}

#[test]
fn special_values_like_std() {
    type Unary = (&'static str, fn(f64) -> f64, fn(f64) -> f64);
    type Binary = (&'static str, fn(f64, f64) -> f64, fn(f64, f64) -> f64);
    let unary: [Unary; 12] = [
        ("exp", exp, f64::exp),
        ("ln", ln, f64::ln),
        ("ln_1p", ln_1p, f64::ln_1p),
        ("sin", sin, f64::sin),
        ("cos", cos, f64::cos),
        ("tan", tan, f64::tan),
        ("atan", atan, f64::atan),
        ("asin", asin, f64::asin),
        ("acos", acos, f64::acos),
        ("cbrt", cbrt, f64::cbrt),
        ("sin_cos.0", |x| sin_cos(x).0, f64::sin),
        ("sin_cos.1", |x| sin_cos(x).1, f64::cos),
    ];
    for (name, ours, reference) in unary {
        for x in SPECIAL {
            assert!(
                special_like_std(ours(x), reference(x)),
                "{name}({x:e}): {:e} vs std {:e}",
                ours(x),
                reference(x)
            );
        }
    }
    let binary: [Binary; 3] =
        [("atan2", atan2, f64::atan2), ("pow", pow, f64::powf), ("hypot", hypot, f64::hypot)];
    for (name, ours, reference) in binary {
        for a in SPECIAL {
            for b in SPECIAL {
                assert!(
                    special_like_std(ours(a, b), reference(a, b)),
                    "{name}({a:e}, {b:e}): {:e} vs std {:e}",
                    ours(a, b),
                    reference(a, b)
                );
            }
        }
    }
}

#[test]
fn special_values_exact() {
    let inf = f64::INFINITY;
    let cases: [(f64, f64); 39] = [
        (exp(0.0), 1.0),
        (exp(-0.0), 1.0),
        (exp(inf), inf),
        (exp(-inf), 0.0),
        (exp(710.0), inf),
        (exp(-746.0), 0.0),
        (ln(1.0), 0.0),
        (ln(0.0), -inf),
        (ln(-0.0), -inf),
        (ln(inf), inf),
        (ln_1p(0.0), 0.0),
        (ln_1p(-0.0), -0.0),
        (ln_1p(-1.0), -inf),
        (ln_1p(inf), inf),
        (sin(0.0), 0.0),
        (sin(-0.0), -0.0),
        (cos(0.0), 1.0),
        (cos(-0.0), 1.0),
        (tan(-0.0), -0.0),
        (atan(-0.0), -0.0),
        (atan(inf), FRAC_PI_2),
        (atan(-inf), -FRAC_PI_2),
        (atan2(0.0, -1.0), PI),
        (atan2(-0.0, -1.0), -PI),
        (atan2(-0.0, 1.0), -0.0),
        (atan2(1.0, 0.0), FRAC_PI_2),
        (asin(1.0), FRAC_PI_2),
        (asin(-0.0), -0.0),
        (acos(1.0), 0.0),
        (acos(-1.0), PI),
        (cbrt(-8.0), -2.0),
        (cbrt(-0.0), -0.0),
        (cbrt(-inf), -inf),
        (pow(f64::NAN, 0.0), 1.0),
        (pow(1.0, f64::NAN), 1.0),
        (pow(-0.0, 3.0), -0.0),
        (pow(-0.0, -3.0), -inf),
        (hypot(3.0, 4.0), 5.0),
        (hypot(inf, f64::NAN), inf),
    ];
    for (i, &(got, want)) in cases.iter().enumerate() {
        assert!(same_bits(got, want), "case {i}: got {got:e}, want {want:e}");
    }
    assert!(pow(-8.0, 1.0 / 3.0).is_nan());
    for x in [f64::NAN, inf, -inf] {
        assert!(sin(x).is_nan() && cos(x).is_nan() && tan(x).is_nan());
    }
    for x in [-1e-300, -1.0, -inf, f64::NAN] {
        assert!(ln(x).is_nan());
    }
    for x in [-1.0 - f64::EPSILON, -inf, f64::NAN] {
        assert!(ln_1p(x).is_nan());
    }
    for x in [1.0 + f64::EPSILON, -2.0, inf, f64::NAN] {
        assert!(asin(x).is_nan() && acos(x).is_nan());
    }
}

/// FNV-1a over the bits of every function on a fixed sample. The constant was computed natively;
/// the same test must pass on wasm32 (bit-identical results across targets).
#[test]
fn bit_exact_fingerprint() {
    let xs = trig_inputs();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |v: f64| {
        for byte in v.to_bits().to_le_bytes() {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for &x in xs.iter().step_by(25) {
        let (s, c) = sin_cos(x);
        for v in [
            exp(x / 64.0),
            ln(x.abs()),
            ln_1p(x.abs()),
            s,
            c,
            tan(x),
            atan(x),
            atan2(x, 0.75),
            asin(x / 1e7),
            acos(x / 1e7),
            cbrt(x),
            pow(x.abs(), 0.37),
            hypot(x, 1.5),
        ] {
            eat(v);
        }
    }
    assert_eq!(h, 0x7fd1_011a_cef6_67d2, "fingerprint changed: {h:#018x}");
}
