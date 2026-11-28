//! Gaussian smoothing and derivatives, computed row by row (docs/ALGORITHME.md §5.1, §5.2).
//!
//! Kernels are symmetric (smoothing) or antisymmetric (derivative) and are applied in the
//! paired form `k_j·(x₋ⱼ + x₊ⱼ)` / `k_j·(x₊ⱼ − x₋ⱼ)`, one tap at a time over whole rows
//! (§18.1, R3): every output value is computed by the same sequence of operations whether the
//! compiler vectorises the loop or not, so the result is bit-identical natively and in WASM.
//! A ring of horizontally filtered rows feeds the vertical pass: memory is a few rows, not a
//! plane. Borders are mirrored (half-sample symmetric).

use super::Plane;
use super::plane::mirror;
use crate::num::math::exp;

/// Below this σ the sampled Gaussian is inaccurate: Lindeberg's discrete kernel is used.
pub const SAMPLED_MIN_SIGMA: f64 = 0.8;

/// A separable Gaussian at scale σ: smoothing taps `g[0..=r]` (g[j] = g(±j)) and derivative
/// taps `d[1..=r]` (the derivative at offset ±j is ∓d[j]; d[0] is unused and 0).
#[derive(Debug, Clone, PartialEq)]
pub struct Kernel {
    pub sigma: f64,
    pub g: Vec<f32>,
    pub d: Vec<f32>,
}

impl Kernel {
    /// # Panics
    /// If `sigma` is not positive.
    pub fn new(sigma: f64) -> Self {
        assert!(sigma > 0.0, "σ must be positive");
        let (g, d) = if sigma >= SAMPLED_MIN_SIGMA { sampled(sigma) } else { discrete(sigma) };
        Kernel { sigma, g: g.iter().map(|&v| v as f32).collect(), d: d.iter().map(|&v| v as f32).collect() }
    }

    pub fn radius(&self) -> usize {
        self.g.len() - 1
    }

    /// Variance of one derivative component for unit white noise: Σd² · Σg² over the 2-D
    /// separable kernel (§5.2), computed on the actual discrete taps.
    pub fn derivative_noise_gain(&self) -> f64 {
        let sum_sq = |v: &[f32], skip_center: bool| -> f64 {
            v.iter()
                .enumerate()
                .map(|(j, &x)| {
                    f64::from(x)
                        * f64::from(x)
                        * if j == 0 { if skip_center { 0.0 } else { 1.0 } } else { 2.0 }
                })
                .sum()
        };
        sum_sq(&self.d, true) * sum_sq(&self.g, false)
    }

    /// Variance of the smoothed value for unit white noise: (Σg²)².
    pub fn smoothing_noise_gain(&self) -> f64 {
        let s: f64 = self
            .g
            .iter()
            .enumerate()
            .map(|(j, &x)| f64::from(x) * f64::from(x) * if j == 0 { 1.0 } else { 2.0 })
            .sum();
        s * s
    }
}

/// Sampled Gaussian, radius ⌈4σ⌉, renormalised so that Σg = 1 and Σ x·g′(x) = −1.
fn sampled(sigma: f64) -> (Vec<f64>, Vec<f64>) {
    let r = (4.0 * sigma).ceil() as usize;
    let g: Vec<f64> = (0..=r).map(|j| exp(-((j * j) as f64) / (2.0 * sigma * sigma))).collect();
    let sum = g[0] + 2.0 * g[1..].iter().sum::<f64>();
    let g: Vec<f64> = g.iter().map(|v| v / sum).collect();
    // g′(x) = −x/σ² g(x); the tap at +j is −d[j], so d[j] ∝ j·g(j) and Σ x·g′ = −2 Σ j·d[j].
    let mut d: Vec<f64> = (0..=r).map(|j| j as f64 * g[j]).collect();
    let moment: f64 = 2.0 * d.iter().enumerate().map(|(j, v)| j as f64 * v).sum::<f64>();
    for v in &mut d {
        *v /= moment;
    }
    (g, d)
}

/// Lindeberg's discrete Gaussian T(n; t) = e^{−t} Iₙ(t) (t = σ²), and the central difference
/// of it as derivative. Exact discrete scale space, valid for small σ.
fn discrete(sigma: f64) -> (Vec<f64>, Vec<f64>) {
    let t = sigma * sigma;
    // Iₙ(t) decays like (t/2)ⁿ/n!: three taps beyond 4σ keep the variance exact to 1e-7.
    let r = (4.0 * sigma).ceil() as usize + 3;
    // Iₙ(t) = Σ_k (t/2)^{2k+n} / (k! (k+n)!), converging fast for t < 1.
    let bessel = |n: usize| {
        let half = t / 2.0;
        let mut term = 1.0;
        for i in 1..=n {
            term *= half / i as f64;
        }
        let mut sum = term;
        for k in 1..60 {
            term *= half * half / (k as f64 * (k + n) as f64);
            sum += term;
            if term < 1e-18 * sum {
                break;
            }
        }
        sum
    };
    let e = exp(-t);
    let g: Vec<f64> = (0..=r).map(|n| e * bessel(n)).collect();
    let sum = g[0] + 2.0 * g[1..].iter().sum::<f64>();
    let g: Vec<f64> = g.iter().map(|v| v / sum).collect();
    // Central difference ½(f(x+1) − f(x−1)) of the smoothed signal: d[j] = ½(g[j−1] − g[j+1]).
    let at = |j: usize| g.get(j).copied().unwrap_or(0.0);
    let mut d: Vec<f64> = (0..=r).map(|j| if j == 0 { 0.0 } else { 0.5 * (at(j - 1) - at(j + 1)) }).collect();
    d.push(0.5 * at(r)); // the difference widens the support by one
    let mut g = g;
    g.push(0.0);
    (g, d)
}

/// Mirrored copy of `row` with `r` extra values on each side.
fn pad(row: &[f32], r: usize, out: &mut Vec<f32>) {
    let w = row.len() as u32;
    out.clear();
    out.extend((0..row.len() + 2 * r).map(|i| row[mirror(i as i64 - r as i64, w) as usize]));
}

/// `out[x] = g0·p[x] + Σ g_j·(p[x−j] + p[x+j])`, with `p` padded by r.
fn smooth_row(p: &[f32], g: &[f32], out: &mut [f32]) {
    let (r, w) = (g.len() - 1, out.len());
    for (o, &v) in out.iter_mut().zip(&p[r..r + w]) {
        *o = g[0] * v;
    }
    for (j, &gj) in g.iter().enumerate().skip(1) {
        for ((o, &a), &b) in out.iter_mut().zip(&p[r - j..r - j + w]).zip(&p[r + j..r + j + w]) {
            *o += gj * (a + b);
        }
    }
}

/// `out[x] = Σ d_j·(p[x+j] − p[x−j])`: the derivative along the row.
fn derive_row(p: &[f32], d: &[f32], out: &mut [f32]) {
    let (r, w) = (d.len() - 1, out.len());
    out.fill(0.0);
    for (j, &dj) in d.iter().enumerate().skip(1) {
        for ((o, &a), &b) in out.iter_mut().zip(&p[r - j..r - j + w]).zip(&p[r + j..r + j + w]) {
            *o += dj * (b - a);
        }
    }
}

/// out += k·(a + b), element by element.
fn add_pair(out: &mut [f32], a: &[f32], b: &[f32], k: f32) {
    for ((o, &p), &q) in out.iter_mut().zip(a).zip(b) {
        *o += k * (p + q);
    }
}

/// out += k·(b − a), element by element.
fn add_diff(out: &mut [f32], a: &[f32], b: &[f32], k: f32) {
    for ((o, &p), &q) in out.iter_mut().zip(a).zip(b) {
        *o += k * (q - p);
    }
}

/// out = k·a.
fn set_scaled(out: &mut [f32], a: &[f32], k: f32) {
    for (o, &p) in out.iter_mut().zip(a) {
        *o = k * p;
    }
}

/// Smoothed value and both derivatives of `src` at scale `kernel`, delivered row by row in
/// increasing y to `sink(y, smooth, d/dx, d/dy)`.
pub fn derivatives(src: &Plane<f32>, kernel: &Kernel, mut sink: impl FnMut(u32, &[f32], &[f32], &[f32])) {
    let (w, h) = (src.width() as usize, src.height());
    let (g, d) = (&kernel.g, &kernel.d);
    debug_assert_eq!(g.len(), d.len());
    let r = g.len() - 1;
    let n = 2 * r + 1;
    // Ring of horizontally filtered rows y−r..=y+r (mirrored): smoothed and x-derivative.
    let mut hs = vec![vec![0.0f32; w]; n];
    let mut hd = vec![vec![0.0f32; w]; n];
    let mut padded = Vec::new();
    let mut fill = |slot: usize, y: i64, hs: &mut [Vec<f32>], hd: &mut [Vec<f32>]| {
        pad(src.row(mirror(y, h)), r, &mut padded);
        smooth_row(&padded, g, &mut hs[slot]);
        derive_row(&padded, d, &mut hd[slot]);
    };
    for k in 0..n {
        fill(k, k as i64 - r as i64, &mut hs, &mut hd);
    }
    let (mut s, mut dx, mut dy) = (vec![0.0f32; w], vec![0.0f32; w], vec![0.0f32; w]);
    for y in 0..h {
        // Ring slot of row y + k (k in −r..=r).
        let slot = |k: i64| (i64::from(y) + k).rem_euclid(n as i64) as usize;
        let c = slot(0);
        set_scaled(&mut s, &hs[c], g[0]);
        set_scaled(&mut dx, &hd[c], g[0]);
        dy.fill(0.0);
        for j in 1..=r {
            let (a, b) = (slot(-(j as i64)), slot(j as i64));
            add_pair(&mut s, &hs[a], &hs[b], g[j]);
            add_pair(&mut dx, &hd[a], &hd[b], g[j]);
            add_diff(&mut dy, &hs[a], &hs[b], d[j]);
        }
        sink(y, &s, &dx, &dy);
        // Replace row y − r by row y + r + 1.
        let next = i64::from(y) + r as i64 + 1;
        fill(next.rem_euclid(n as i64) as usize, next, &mut hs, &mut hd);
    }
}

/// `src` smoothed at scale `kernel`.
pub fn smooth(src: &Plane<f32>, kernel: &Kernel) -> Plane<f32> {
    let mut out = Plane::new(src.width(), src.height(), 0.0);
    let (w, h) = (src.width() as usize, src.height());
    let g = &kernel.g;
    let r = g.len() - 1;
    let n = 2 * r + 1;
    let mut ring = vec![vec![0.0f32; w]; n];
    let mut padded = Vec::new();
    let mut fill = |slot: usize, y: i64, ring: &mut [Vec<f32>]| {
        pad(src.row(mirror(y, h)), r, &mut padded);
        smooth_row(&padded, g, &mut ring[slot]);
    };
    for k in 0..n {
        fill(k, k as i64 - r as i64, &mut ring);
    }
    for y in 0..h {
        let slot = |k: i64| (i64::from(y) + k).rem_euclid(n as i64) as usize;
        let row = out.row_mut(y);
        set_scaled(row, &ring[slot(0)], g[0]);
        for (j, &gj) in g.iter().enumerate().skip(1) {
            add_pair(row, &ring[slot(-(j as i64))], &ring[slot(j as i64)], gj);
        }
        let next = i64::from(y) + r as i64 + 1;
        fill(next.rem_euclid(n as i64) as usize, next, &mut ring);
    }
    out
}
