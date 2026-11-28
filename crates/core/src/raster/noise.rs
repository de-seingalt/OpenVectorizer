//! Noise model of the image (docs/ALGORITHME.md §4.3).
//!
//! Per channel, a noise level function measured on the encoded values by robust Immerkær
//! estimates per intensity class over the flat, unclipped pixels; a floor that keeps flat digital
//! art from having zero noise; and a map of local factors over tiles for what the level function
//! does not describe (scan grain, uneven compression). Noise in Oklab follows by propagation
//! through the Jacobian of the conversion.

use super::Plane;
use super::oklab::{decode_slope, jacobian};
use crate::num::math::pow;

/// Noise floor in 8-bit sRGB code units (×1/255), raised for JPEG input.
pub const FLOOR_CODES: f64 = 0.5;
pub const JPEG_FLOOR_CODES: f64 = 2.0;
/// Intensity classes of the level function.
pub const CLASSES: usize = 16;
/// Minimum flat samples for a class (or a tile) to be used.
pub const MIN_CLASS_SAMPLES: usize = 64;
pub const MIN_TILE_SAMPLES: usize = 256;
pub const TILE: u32 = 64;
/// |Immerkær response| in codes is at most 16 × 255.
const RESPONSE_BINS: usize = 16 * 255 + 1;
/// Bounds of the local factor.
pub const FACTOR_RANGE: (f32, f32) = (0.25, 4.0);

/// Immerkær's mask N has Σ N² = 36; median(|r|) = 0.6745 · 6 · σ for Gaussian noise.
const MAD_TO_SIGMA: f64 = 1.0 / (0.674_489_75 * 6.0);

/// Noise of one channel as a function of its value: a standard deviation measured on the
/// encoded values (8-bit codes / 255) per intensity class, interpolated linearly between the
/// classes' mean values (no parametric model: tone curves and compression bend any simple
/// law), plus a floor. Quantization, JPEG and scanner noise live in the encoded values; the
/// noise in linear light follows from the slope of the decoding curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LevelFunction {
    /// Mean encoded value of each class with enough flat samples, and its noise; classes
    /// without samples have `NaN` and are skipped by the interpolation.
    pub centers: [f64; CLASSES],
    pub sigmas: [f64; CLASSES],
    /// Floor in encoded units (fraction of the full range).
    pub floor: f64,
    /// Whether the channel is gamma-encoded sRGB (colour) or linear (alpha).
    pub srgb: bool,
}

impl LevelFunction {
    fn empty(floor: f64, srgb: bool) -> Self {
        LevelFunction { centers: [f64::NAN; CLASSES], sigmas: [f64::NAN; CLASSES], floor, srgb }
    }

    /// Noise measured at encoded value `v`, without floor (0 if nothing was measured).
    pub fn model(&self, v: f64) -> f64 {
        let known = || self.centers.iter().zip(&self.sigmas).filter(|(c, _)| !c.is_nan());
        let mut below: Option<(f64, f64)> = None;
        let mut above: Option<(f64, f64)> = None;
        for (&c, &s) in known() {
            if c <= v {
                below = Some((c, s));
            } else if above.is_none() {
                above = Some((c, s));
            }
        }
        match (below, above) {
            (Some((c0, s0)), Some((c1, s1))) => s0 + (s1 - s0) * (v - c0) / (c1 - c0),
            (Some((_, s)), None) | (None, Some((_, s))) => s,
            (None, None) => 0.0,
        }
    }

    /// Noise of the encoded value `c`, times the local `factor`, with the floor.
    pub fn sigma_encoded(&self, c: f64, factor: f64) -> f64 {
        (self.model(c) * factor).max(self.floor)
    }

    /// Slope of linear light with respect to the encoded value at linear value `v`.
    fn slope(&self, v: f64) -> f64 {
        if self.srgb { decode_slope(encode(v)) } else { 1.0 }
    }

    /// The floor in linear light at linear value `v`.
    pub fn floor_at(&self, v: f64) -> f64 {
        self.floor * self.slope(v)
    }

    /// Noise standard deviation in linear light at linear value `v`, times the local `factor`.
    pub fn sigma(&self, v: f64, factor: f64) -> f64 {
        let c = if self.srgb { encode(v) } else { v };
        self.sigma_encoded(c, factor) * self.slope(v)
    }
}

/// sRGB encoding of a linear value (inverse of `decode`).
fn encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * pow(v, 1.0 / 2.4) - 0.055 }
}

/// The noise of an image.
#[derive(Debug, Clone, PartialEq)]
pub struct NoiseModel {
    /// R, G, B (measured on the encoded values, see [`LevelFunction`]).
    pub channels: [LevelFunction; 3],
    /// Alpha, when it is not constant.
    pub alpha: Option<LevelFunction>,
    tiles_w: u32,
    tiles_h: u32,
    factors: Vec<f32>,
}

impl NoiseModel {
    /// Local factor at SVG position (x, y), interpolated bilinearly between tile centres.
    pub fn factor(&self, x: f64, y: f64) -> f64 {
        let t = f64::from(TILE);
        let fx = (x / t - 0.5).clamp(0.0, f64::from(self.tiles_w - 1));
        let fy = (y / t - 0.5).clamp(0.0, f64::from(self.tiles_h - 1));
        let (x0, y0) = (fx.floor() as u32, fy.floor() as u32);
        let (x1, y1) = ((x0 + 1).min(self.tiles_w - 1), (y0 + 1).min(self.tiles_h - 1));
        let (tx, ty) = (fx - f64::from(x0), fy - f64::from(y0));
        let f = |i: u32, j: u32| f64::from(self.factors[(j * self.tiles_w + i) as usize]);
        let top = f(x0, y0) * (1.0 - tx) + f(x1, y0) * tx;
        let bottom = f(x0, y1) * (1.0 - tx) + f(x1, y1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// Noise covariance of (L, a, b) at a pixel of linear colour `rgb` and SVG position (x, y):
    /// J · diag(σ²) · Jᵀ.
    pub fn oklab_covariance(&self, rgb: [f64; 3], x: f64, y: f64) -> [[f64; 3]; 3] {
        let k = self.factor(x, y);
        let var = [0, 1, 2].map(|c| {
            let s = self.channels[c].sigma(rgb[c], k);
            s * s
        });
        let j = jacobian(rgb);
        let mut out = [[0.0; 3]; 3];
        for (i, row) in out.iter_mut().enumerate() {
            for (m, o) in row.iter_mut().enumerate() {
                *o = (0..3).map(|c| j[i][c] * var[c] * j[m][c]).sum();
            }
        }
        out
    }

    /// Estimate the noise of encoded R, G, B planes (codes / 255) and of alpha, if given and
    /// not constant.
    pub fn estimate(encoded: &[Plane<f32>; 3], alpha: Option<&Plane<f32>>, jpeg: bool) -> Self {
        let floor = if jpeg { JPEG_FLOOR_CODES } else { FLOOR_CODES } / 255.0;
        let alpha = alpha.filter(|a| a.data().iter().any(|&v| v.to_bits() != a.data()[0].to_bits()));
        let mut planes: Vec<&Plane<f32>> = encoded.iter().collect();
        planes.extend(alpha);
        let flat = flat_mask(&planes);
        let fit = |p: &Plane<f32>, srgb: bool| {
            fit_level(p, &flat, LevelFunction::empty(if srgb { floor } else { FLOOR_CODES / 255.0 }, srgb))
        };
        let channels = [fit(&encoded[0], true), fit(&encoded[1], true), fit(&encoded[2], true)];
        let alpha_fn = alpha.map(|a| fit(a, false));
        let (tiles_w, tiles_h, factors) = tile_factors(encoded, &flat, &channels);
        NoiseModel { channels, alpha: alpha_fn, tiles_w, tiles_h, factors }
    }
}

/// Immerkær response at interior pixel (x, y).
#[inline]
fn immerkaer(p: &Plane<f32>, x: u32, y: u32) -> f32 {
    let (up, mid, down) = (p.row(y - 1), p.row(y), p.row(y + 1));
    let (a, b, c) = (x as usize - 1, x as usize, x as usize + 1);
    (up[a] - 2.0 * up[b] + up[c]) - 2.0 * (mid[a] - 2.0 * mid[b] + mid[c])
        + (down[a] - 2.0 * down[b] + down[c])
}

#[inline]
fn local_mean(p: &Plane<f32>, x: u32, y: u32) -> f32 {
    let mut s = 0.0;
    for yy in y - 1..=y + 1 {
        let row = p.row(yy);
        s += row[x as usize - 1] + row[x as usize] + row[x as usize + 1];
    }
    s / 9.0
}

/// Whether the 3×3 neighbourhood touches the ends of the range, where noise is clipped (a white
/// background at 255 has no noise to measure, whatever the noise of the image).
#[inline]
fn clipped(p: &Plane<f32>, x: u32, y: u32) -> bool {
    (y - 1..=y + 1).any(|yy| p.row(yy)[x as usize - 1..=x as usize + 1].iter().any(|&v| v <= 0.0 || v >= 1.0))
}

/// Interior pixels whose summed absolute central differences are at most their median: the
/// flat pixels, where edges do not inflate the estimate.
fn flat_mask(planes: &[&Plane<f32>]) -> Plane<bool> {
    let (w, h) = (planes[0].width(), planes[0].height());
    let mut mask = Plane::new(w, h, false);
    if w < 3 || h < 3 {
        return mask;
    }
    let mut grad = Plane::new(w, h, f32::INFINITY);
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            let mut g = 0.0;
            for p in planes {
                g += (p.get(x + 1, y) - p.get(x - 1, y)).abs() + (p.get(x, y + 1) - p.get(x, y - 1)).abs();
            }
            grad.set(x, y, g);
        }
    }
    let mut values: Vec<f32> = grad.data().iter().copied().filter(|v| v.is_finite()).collect();
    let median = order_statistic(&mut values, 0.5);
    for (m, &g) in mask.data_mut().iter_mut().zip(grad.data()) {
        *m = g <= median;
    }
    mask
}

/// The q-quantile of `values` (an order statistic: the same value whatever the algorithm).
fn order_statistic(values: &mut [f32], q: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let k = ((values.len() - 1) as f64 * q).round() as usize;
    *values.select_nth_unstable_by(k, f32::total_cmp).1
}

fn fit_level(p: &Plane<f32>, flat: &Plane<bool>, mut lf: LevelFunction) -> LevelFunction {
    let (w, h) = (p.width(), p.height());
    // Per class: histogram of |r| in code units (an integer for 8-bit input), sum of values.
    let mut by_class: Vec<(Vec<u32>, f64)> = vec![(vec![0; RESPONSE_BINS], 0.0); CLASSES];
    if w >= 3 && h >= 3 {
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                if !flat.get(x, y) || clipped(p, x, y) {
                    continue;
                }
                let m = local_mean(p, x, y);
                let class = ((m * CLASSES as f32) as usize).min(CLASSES - 1);
                let r = (immerkaer(p, x, y).abs() * 255.0).round() as usize;
                by_class[class].0[r.min(RESPONSE_BINS - 1)] += 1;
                by_class[class].1 += f64::from(m);
            }
        }
    }
    // Raw estimate per class, then a light smoothing of σ² over neighbouring classes weighted by
    // their sample counts: each class alone is too noisy to trust. (No monotonicity: noise that
    // grows with light in linear values falls with the value once encoded.)
    let mut known: Vec<(usize, f64, f64)> = Vec::new(); // (class, σ², samples)
    for (k, (hist, sum)) in by_class.iter().enumerate() {
        let n: u64 = hist.iter().map(|&c| u64::from(c)).sum();
        if n >= MIN_CLASS_SAMPLES as u64 {
            lf.centers[k] = *sum / n as f64;
            let s = grouped_median(hist) / 255.0 * MAD_TO_SIGMA;
            known.push((k, s * s, n as f64));
        }
    }
    for i in 0..known.len() {
        let (mut num, mut den) = (0.0, 0.0);
        for (j, &(_, v, n)) in known.iter().enumerate() {
            let wt = match i.abs_diff(j) {
                0 => 2.0,
                1 => 1.0,
                _ => 0.0,
            } * n;
            num += wt * v;
            den += wt;
        }
        lf.sigmas[known[i].0] = (num / den).sqrt();
    }
    lf
}

/// Median of integer data given as a histogram, interpolated within the median's unit bin
/// [k − ½, k + ½] (median of grouped data): the plain median of quantized responses moves by
/// whole codes, far too coarse for noise of about one code.
fn grouped_median(hist: &[u32]) -> f64 {
    let n: u64 = hist.iter().map(|&c| u64::from(c)).sum();
    let half = n as f64 / 2.0;
    let mut below = 0.0;
    for (k, &c) in hist.iter().enumerate() {
        let c = f64::from(c);
        if c > 0.0 && below + c >= half {
            let (lower, width) = if k == 0 { (0.0, 0.5) } else { (k as f64 - 0.5, 1.0) };
            return lower + (half - below) / c * width;
        }
        below += c;
    }
    0.0
}

/// Local factors: per tile, the median ratio of the observed flat responses to the level
/// function's prediction (1 where there are too few flat pixels).
fn tile_factors(
    encoded: &[Plane<f32>; 3],
    flat: &Plane<bool>,
    channels: &[LevelFunction; 3],
) -> (u32, u32, Vec<f32>) {
    let (w, h) = (encoded[0].width(), encoded[0].height());
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut factors = vec![1.0f32; (tw * th) as usize];
    let mut ratios = Vec::new();
    for ty in 0..th {
        for tx in 0..tw {
            ratios.clear();
            let (x0, y0) = ((tx * TILE).max(1), (ty * TILE).max(1));
            let (x1, y1) = (((tx + 1) * TILE).min(w - 1), ((ty + 1) * TILE).min(h - 1));
            for y in y0..y1 {
                for x in x0..x1 {
                    if !flat.get(x, y) {
                        continue;
                    }
                    for (p, lf) in encoded.iter().zip(channels) {
                        if clipped(p, x, y) {
                            continue;
                        }
                        let model = lf.model(f64::from(local_mean(p, x, y)));
                        if model > 0.0 {
                            ratios.push((f64::from(immerkaer(p, x, y).abs()) * MAD_TO_SIGMA / model) as f32);
                        }
                    }
                }
            }
            if ratios.len() >= MIN_TILE_SAMPLES {
                let f = order_statistic(&mut ratios, 0.5);
                factors[(ty * tw + tx) as usize] = f.clamp(FACTOR_RANGE.0, FACTOR_RANGE.1);
            }
        }
    }
    (tw, th, factors)
}
