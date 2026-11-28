//! Degradations of a rendered scene: optical blur, noise, upscaling. All deterministic.

use crate::num::math::{exp, ln, sin_cos};
use crate::num::rng::Pcg32;
use crate::raster::Plane;
use std::f64::consts::PI;

/// Normalized Gaussian kernel of standard deviation `sigma`, radius ⌈4σ⌉.
fn kernel(sigma: f64) -> Vec<f64> {
    let r = (4.0 * sigma).ceil() as i64;
    let mut k: Vec<f64> = (-r..=r).map(|i| exp(-((i * i) as f64) / (2.0 * sigma * sigma))).collect();
    let sum: f64 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    k
}

fn blur_plane(p: &Plane<f64>, k: &[f64]) -> Plane<f64> {
    let r = (k.len() / 2) as i64;
    let (w, h) = (p.width(), p.height());
    let mut tmp = Plane::new(w, h, 0.0);
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, &kj) in k.iter().enumerate() {
                s += kj * p.get_mirrored(i64::from(x) + j as i64 - r, i64::from(y));
            }
            tmp.set(x, y, s);
        }
    }
    let mut out = Plane::new(w, h, 0.0);
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (j, &kj) in k.iter().enumerate() {
                s += kj * tmp.get_mirrored(i64::from(x), i64::from(y) + j as i64 - r);
            }
            out.set(x, y, s);
        }
    }
    out
}

/// Gaussian blur of standard deviation `sigma` px (mirrored borders), on premultiplied colours.
pub fn blur(planes: &[Plane<f64>; 4], sigma: f64) -> [Plane<f64>; 4] {
    if sigma <= 0.0 {
        return planes.clone();
    }
    let k = kernel(sigma);
    let alpha = &planes[3];
    let pre: Vec<Plane<f64>> = planes[..3]
        .iter()
        .map(|c| {
            let mut m = c.clone();
            for (v, &a) in m.data_mut().iter_mut().zip(alpha.data()) {
                *v *= a;
            }
            blur_plane(&m, &k)
        })
        .collect();
    let a = blur_plane(alpha, &k);
    let un = |p: &Plane<f64>| {
        let mut m = p.clone();
        for (v, &a) in m.data_mut().iter_mut().zip(a.data()) {
            *v = if a > 0.0 { *v / a } else { 0.0 };
        }
        m
    };
    [un(&pre[0]), un(&pre[1]), un(&pre[2]), a]
}

/// Add white Gaussian noise of standard deviation `sigma` (in [0, 1] units) to the colours.
pub fn noise(planes: &[Plane<f64>; 4], sigma: f64, seed: u64) -> [Plane<f64>; 4] {
    let mut rng = Pcg32::new(seed, 0x006e_6f69_7365);
    let mut out = planes.clone();
    for p in &mut out[..3] {
        for v in p.data_mut() {
            *v += sigma * gaussian(&mut rng);
        }
    }
    out
}

/// Standard normal sample (Box–Muller).
pub fn gaussian(rng: &mut Pcg32) -> f64 {
    let u1 = 1.0 - rng.next_f64(); // in (0, 1]
    let u2 = rng.next_f64();
    let (_, c) = sin_cos(2.0 * PI * u2);
    (-2.0 * ln(u1)).sqrt() * c
}

/// Upscale by an integer factor, repeating pixels.
pub fn upscale_nearest(planes: &[Plane<f64>; 4], factor: u32) -> [Plane<f64>; 4] {
    planes.clone().map(|p| {
        let mut out = Plane::new(p.width() * factor, p.height() * factor, 0.0);
        for y in 0..out.height() {
            for x in 0..out.width() {
                out.set(x, y, p.get(x / factor, y / factor));
            }
        }
        out
    })
}

/// Upscale by an integer factor with bilinear interpolation between pixel centres.
pub fn upscale_bilinear(planes: &[Plane<f64>; 4], factor: u32) -> [Plane<f64>; 4] {
    let f = f64::from(factor);
    planes.clone().map(|p| {
        let mut out = Plane::new(p.width() * factor, p.height() * factor, 0.0);
        for y in 0..out.height() {
            for x in 0..out.width() {
                let sx = (f64::from(x) + 0.5) / f - 0.5;
                let sy = (f64::from(y) + 0.5) / f - 0.5;
                let (x0, y0) = (sx.floor(), sy.floor());
                let (tx, ty) = (sx - x0, sy - y0);
                let at = |dx: i64, dy: i64| p.get_mirrored(x0 as i64 + dx, y0 as i64 + dy);
                let top = at(0, 0) * (1.0 - tx) + at(1, 0) * tx;
                let bottom = at(0, 1) * (1.0 - tx) + at(1, 1) * tx;
                out.set(x, y, top * (1.0 - ty) + bottom * ty);
            }
        }
        out
    })
}
