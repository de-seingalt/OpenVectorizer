//! Image pyramid (docs/ALGORITHME.md §2, §5.1; §18.1 R4).
//!
//! Level o + 1 is level o smoothed by a Gaussian of σ = `PRE_SIGMA` (in level-o pixels) and
//! averaged over 2×2 blocks, both in one pass: the combined kernel h = g ∗ [½, ½] is evaluated
//! only at the kept positions. Pixel i of level o covers [2^o·i, 2^o·(i+1)] in SVG coordinates,
//! so its centre is at 2^o·(i + ½). `variance` records the blur added by the pyramid, in
//! original pixels², for scale selection.

use super::Plane;
use super::plane::mirror;
use crate::num::math::exp;

/// Anti-alias blur before each halving (level-o pixels).
pub const PRE_SIGMA: f64 = 1.0;

/// Levels stop when the smaller side would drop below this.
pub const MIN_SIDE: u32 = 16;

/// Taps h[m] for m = 1..=R+1 of the symmetric-about-½ kernel g ∗ [½, ½]: output i of a row is
/// `Σ_m h[m]·(p[2i+m] + p[2i+1−m])`. Also returns the kernel's variance (level-o pixels²).
fn halving_taps(sigma: f64) -> (Vec<f32>, f64) {
    let r = (4.0 * sigma).ceil() as i64;
    let g: Vec<f64> = (-r..=r).map(|j| exp(-((j * j) as f64) / (2.0 * sigma * sigma))).collect();
    let sum: f64 = g.iter().sum();
    let at = |j: i64| if j.abs() <= r { g[(j + r) as usize] / sum } else { 0.0 };
    // h(m) = ½(g(m) + g(m−1)) for m in −r..=r+1, symmetric about ½: h(m) = h(1−m).
    let taps: Vec<f64> = (1..=r + 1).map(|m| f64::midpoint(at(m), at(m - 1))).collect();
    // Variance about ½: 2·Σ_{m≥1} h(m)·(m − ½)².
    let var =
        2.0 * taps.iter().enumerate().map(|(k, &v)| v * (k as f64 + 0.5) * (k as f64 + 0.5)).sum::<f64>();
    (taps.iter().map(|&v| v as f32).collect(), var)
}

/// Level-o plane halved: width and height are divided by 2 (rounded down).
pub fn halve(src: &Plane<f32>) -> Plane<f32> {
    let (taps, _) = halving_taps(PRE_SIGMA);
    let (w, h) = (src.width(), src.height());
    let (w2, h2) = (w / 2, h / 2);
    let r = taps.len() as i64;
    // Horizontal pass on every source row, keeping even positions only.
    let mut rows = Plane::new(w2, h, 0.0f32);
    let mut padded = Vec::new();
    for y in 0..h {
        let row = src.row(y);
        padded.clear();
        padded.extend((-r..i64::from(w) + r).map(|i| row[mirror(i, w) as usize]));
        let out = rows.row_mut(y);
        out.fill(0.0);
        for (m, &hm) in taps.iter().enumerate() {
            let m = m as i64 + 1;
            for (i, o) in out.iter_mut().enumerate() {
                let base = 2 * i as i64 + r;
                *o += hm * (padded[(base + m) as usize] + padded[(base + 1 - m) as usize]);
            }
        }
    }
    // Vertical pass at even rows.
    let mut out = Plane::new(w2, h2, 0.0f32);
    for y in 0..h2 {
        let dst = out.row_mut(y);
        for (m, &hm) in taps.iter().enumerate() {
            let m = m as i64 + 1;
            let a = rows.row(mirror(2 * i64::from(y) + m, h));
            let b = rows.row(mirror(2 * i64::from(y) + 1 - m, h));
            for ((o, &p), &q) in dst.iter_mut().zip(a).zip(b) {
                *o += hm * (p + q);
            }
        }
    }
    out
}

/// A pyramid of one channel: `levels[0]` is the source.
#[derive(Debug, Clone)]
pub struct Pyramid {
    pub levels: Vec<Plane<f32>>,
    /// Blur variance added by the pyramid at each level, in original pixels² (0 at level 0).
    pub variance: Vec<f64>,
}

impl Pyramid {
    pub fn new(base: Plane<f32>) -> Self {
        let (_, var) = halving_taps(PRE_SIGMA);
        let mut levels = vec![base];
        let mut variance = vec![0.0];
        while levels.last().is_some_and(|l| l.width().min(l.height()) / 2 >= MIN_SIDE) {
            let o = levels.len() - 1;
            let next = halve(&levels[o]);
            variance.push(variance[o] + var * f64::from(1u32 << (2 * o)));
            levels.push(next);
        }
        Pyramid { levels, variance }
    }

    /// SVG coordinate of the centre of pixel `i` at level `o`.
    pub fn center(o: usize, i: u32) -> f64 {
        f64::from(1u32 << o) * (f64::from(i) + 0.5)
    }
}
