//! Clean-up of compression artifacts before the colours are analysed: an edge-preserving
//! (bilateral) filter in Oklab. Each pixel becomes the mean of its neighbours weighted by
//! distance and by colour similarity: JPEG blocks, ringing halos and noise are smoothed, edges
//! between different colours are not (their pixels are too far apart in colour to mix).

use crate::RgbaImage;
use crate::color::{Lab, oklab_to_srgb, srgb_to_oklab};
use crate::num::math::exp;

/// Neighbours whose opacity differs more than this are ignored (transparency edges stay).
const ALPHA_SLACK: i32 = 64;

/// Entries of the colour-weight table, which covers squared distances up to `RANGE_SIGMAS²/2`
/// in units of `σ_c²` (weights beyond are below `e^-4.5` ≈ 1 % and are dropped).
const RANGE_STEPS: usize = 4096;
const RANGE_SIGMAS: f32 = 5.0;

/// Side of the tiles tested for flatness.
const TILE: usize = 8;

/// The filtered image for `strength` in 0..=1 (`None` at 0: nothing to do).
pub fn denoise(image: &RgbaImage, strength: f32) -> Option<RgbaImage> {
    if strength <= 0.0 {
        return None;
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data();
    let lab: Vec<Lab> = data.as_chunks::<4>().0.iter().map(|p| srgb_to_oklab([p[0], p[1], p[2]])).collect();
    // Radius 1..=3 px; colour scale from 0.015 (only near-identical tints mix) to 0.065.
    let radius = 1 + (strength * 2.0).round() as usize;
    let sigma_c = 0.015 + 0.05 * strength;
    let sigma_s = radius as f32 / 1.5;
    let r = radius as i64;
    let spatial: Vec<f32> = (-r..=r)
        .flat_map(|dy| (-r..=r).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| exp(-((dx * dx + dy * dy) as f64) / f64::from(2.0 * sigma_s * sigma_s)) as f32)
        .collect();
    // Colour weight exp(−d²/2σ_c²), tabulated on d² (bin centres): no exponential per pixel.
    let d2_max = RANGE_SIGMAS * RANGE_SIGMAS * sigma_c * sigma_c;
    let bin = d2_max / RANGE_STEPS as f32;
    let range: Vec<f32> = (0..RANGE_STEPS)
        .map(|k| exp(-f64::from((k as f32 + 0.5) * bin) / f64::from(2.0 * sigma_c * sigma_c)) as f32)
        .collect();
    let inv_bin = 1.0 / bin;
    let busy = busy_tiles(data, w, h, radius);
    let tiles_w = w.div_ceil(TILE);
    let mut out = data.to_vec();
    let filter = |x: usize, y: usize, checked: bool| -> Option<[u8; 3]> {
        let i = y * w + x;
        let alpha = i32::from(data[i * 4 + 3]);
        if alpha == 0 {
            return None;
        }
        let c = lab[i];
        let (mut sum, mut total) = (Lab::default(), 0.0f32);
        let mut k = 0;
        for dy in -r..=r {
            for dx in -r..=r {
                let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                let weight_s = spatial[k];
                k += 1;
                if checked && (nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64) {
                    continue;
                }
                let j = ny as usize * w + nx as usize;
                if (i32::from(data[j * 4 + 3]) - alpha).abs() > ALPHA_SLACK {
                    continue;
                }
                let step = (c.dist2(lab[j]) * inv_bin) as usize;
                if step >= RANGE_STEPS {
                    continue;
                }
                let weight = weight_s * range[step];
                sum = sum.add(lab[j].scale(weight));
                total += weight;
            }
        }
        Some(oklab_to_srgb(sum.scale(1.0 / total)))
    };
    for y in 0..h {
        let inner_y = y >= radius && y + radius < h;
        for x in 0..w {
            // A pixel whose whole neighbourhood has its colour keeps it exactly.
            if !busy[(y / TILE) * tiles_w + x / TILE] {
                continue;
            }
            let checked = !(inner_y && x >= radius && x + radius < w);
            if let Some(rgb) = filter(x, y, checked) {
                out[(y * w + x) * 4..][..3].copy_from_slice(&rgb);
            }
        }
    }
    RgbaImage::new(image.width(), image.height(), out).ok()
}

/// For each `TILE × TILE` tile, whether some pixel within `radius` of it differs from the
/// others (flat tiles are left untouched: filtering identical pixels changes nothing).
fn busy_tiles(data: &[u8], w: usize, h: usize, radius: usize) -> Vec<bool> {
    let (tw, th) = (w.div_ceil(TILE), h.div_ceil(TILE));
    let mut busy = vec![false; tw * th];
    for ty in 0..th {
        for tx in 0..tw {
            let (x0, y0) = ((tx * TILE).saturating_sub(radius), (ty * TILE).saturating_sub(radius));
            let (x1, y1) = (((tx + 1) * TILE + radius).min(w), ((ty + 1) * TILE + radius).min(h));
            let first = &data[(y0 * w + x0) * 4..][..4];
            busy[ty * tw + tx] = (y0..y1).any(|y| {
                data[(y * w + x0) * 4..(y * w + x1) * 4].as_chunks::<4>().0.iter().any(|p| p != first)
            });
        }
    }
    busy
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smooths_noise_but_keeps_edges() {
        // Left half dark red, right half white, with ±6 noise in a checker pattern.
        let (w, h) = (16u32, 8u32);
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let n: i16 = if (x + y) % 2 == 0 { 6 } else { -6 };
                let base: [i16; 3] = if x < 8 { [150, 30, 30] } else { [245, 245, 245] };
                for v in base {
                    data.push((v + n).clamp(0, 255) as u8);
                }
                data.push(255);
            }
        }
        let img = RgbaImage::new(w, h, data).expect("image");
        let out = denoise(&img, 0.5).expect("filtered");
        let px = |x: u32, y: u32| {
            let i = ((y * w + x) * 4) as usize;
            [out.data()[i], out.data()[i + 1], out.data()[i + 2]]
        };
        // Noise mostly gone inside each half...
        let (a, b) = (px(3, 3), px(4, 3));
        assert!((i16::from(a[0]) - i16::from(b[0])).abs() <= 4, "{a:?} {b:?}");
        // ...and the edge is still sharp: no pink pixels next to it.
        assert!(px(7, 3)[1] < 60 && px(8, 3)[1] > 220, "{:?} {:?}", px(7, 3), px(8, 3));
    }
}
