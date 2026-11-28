//! Clean-up of compression artifacts before the colours are analysed: an edge-preserving
//! (bilateral) filter in Oklab. Each pixel becomes the mean of its neighbours weighted by
//! distance and by colour similarity: JPEG blocks, ringing halos and noise are smoothed, edges
//! between different colours are not (their pixels are too far apart in colour to mix).

use crate::RgbaImage;
use crate::color::{Lab, oklab_to_srgb, srgb_to_oklab};

/// Neighbours whose opacity differs more than this are ignored (transparency edges stay).
const ALPHA_SLACK: i32 = 64;

/// The filtered image for `strength` in 0..=1 (`None` at 0: nothing to do).
pub fn denoise(image: &RgbaImage, strength: f32) -> Option<RgbaImage> {
    if strength <= 0.0 {
        return None;
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data();
    let lab: Vec<Lab> = data.as_chunks::<4>().0.iter().map(|p| srgb_to_oklab([p[0], p[1], p[2]])).collect();
    // Radius 1..=3 px; colour scale from 0.015 (only near-identical tints mix) to 0.065.
    let radius = 1 + (strength * 2.0).round() as i64;
    let sigma_c = 0.015 + 0.05 * strength;
    let sigma_s = radius as f32 / 1.5;
    let spatial: Vec<f32> = (-radius..=radius)
        .flat_map(|dy| (-radius..=radius).map(move |dx| (dx, dy)))
        .map(|(dx, dy)| (-((dx * dx + dy * dy) as f32) / (2.0 * sigma_s * sigma_s)).exp())
        .collect();
    let inv_c = 1.0 / (2.0 * sigma_c * sigma_c);
    let mut out = data.to_vec();
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let i = (y as usize) * w + x as usize;
            let alpha = i32::from(data[i * 4 + 3]);
            if alpha == 0 {
                continue;
            }
            let c = lab[i];
            let (mut sum, mut total) = (Lab::default(), 0.0f32);
            let mut k = 0;
            for dy in -radius..=radius {
                for dx in -radius..=radius {
                    let (nx, ny) = (x + dx, y + dy);
                    let weight_s = spatial[k];
                    k += 1;
                    if nx < 0 || ny < 0 || nx >= w as i64 || ny >= h as i64 {
                        continue;
                    }
                    let j = (ny as usize) * w + nx as usize;
                    if (i32::from(data[j * 4 + 3]) - alpha).abs() > ALPHA_SLACK {
                        continue;
                    }
                    let weight = weight_s * (-c.dist2(lab[j]) * inv_c).exp();
                    sum = sum.add(lab[j].scale(weight));
                    total += weight;
                }
            }
            let [r, g, b] = oklab_to_srgb(sum.scale(1.0 / total));
            out[i * 4..i * 4 + 3].copy_from_slice(&[r, g, b]);
        }
    }
    RgbaImage::new(image.width(), image.height(), out).ok()
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
