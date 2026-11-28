//! Edge sharpness: the hybrid part of the boundary detection. Region boundaries come from the
//! colour labels (exact, shared, watertight), but a label boundary is not always an edge of the
//! drawing: inside a gradient, it is wherever the quantization cut the ramp. The image gradient
//! (Scharr filter in Oklab) tells them apart.
//!
//! For a crack between two regions whose colours differ by C, a real edge (a step, anti-aliased
//! over 1–2 px) has a gradient peak of about C/2 at the boundary; a band boundary inside a ramp
//! has the ramp's gradient, C/B for bands B px wide. The sharpness 2·G/C is then ~1 for a real
//! edge and small (2/B) for a band boundary.

use crate::RgbaImage;
use crate::color::{Lab, srgb_to_oklab};
use crate::palette::{Quantized, TRANSPARENT};

pub struct EdgeMap {
    w: usize,
    h: usize,
    /// Gradient magnitude (Oklab units per px) at each pixel.
    grad: Vec<f32>,
    /// Colour of each label (mean of its source colours), in Oklab.
    label: Vec<Lab>,
}

impl EdgeMap {
    pub fn new(image: &RgbaImage, q: &Quantized) -> Self {
        let (w, h) = (image.width() as usize, image.height() as usize);
        let lab: Vec<Lab> =
            image.data().as_chunks::<4>().0.iter().map(|p| srgb_to_oklab([p[0], p[1], p[2]])).collect();
        let at = |x: usize, y: usize| lab[y.min(h - 1) * w + x.min(w - 1)];
        let mut grad = vec![0.0f32; w * h];
        for y in 0..h {
            for x in 0..w {
                let (xm, xp, ym, yp) = (x.saturating_sub(1), x + 1, y.saturating_sub(1), y + 1);
                // Scharr: [3 10 3] smoothing, central difference; normalized so a unit step
                // between two pixels gives 0.5 on each side.
                let d = |a: Lab, b: Lab, k: f32| [(a.l - b.l) * k, (a.a - b.a) * k, (a.b - b.b) * k];
                let mut gx = [0.0f32; 3];
                let mut gy = [0.0f32; 3];
                for (o, k) in [(-1i64, 3.0), (0, 10.0), (1, 3.0)] {
                    let yy = (y as i64 + o).clamp(0, h as i64 - 1) as usize;
                    let xx = (x as i64 + o).clamp(0, w as i64 - 1) as usize;
                    let dx = d(at(xp, yy), at(xm, yy), k / 32.0);
                    let dy = d(at(xx, yp), at(xx, ym), k / 32.0);
                    for c in 0..3 {
                        gx[c] += dx[c];
                        gy[c] += dy[c];
                    }
                }
                grad[y * w + x] = (0..3).map(|c| gx[c] * gx[c] + gy[c] * gy[c]).sum::<f32>().sqrt();
            }
        }
        let label = q
            .model
            .iter()
            .map(|sources| {
                let k = 1.0 / sources.len().max(1) as f32;
                sources.iter().fold(Lab::default(), |acc, &c| acc.add(srgb_to_oklab(c).scale(k)))
            })
            .collect();
        EdgeMap { w, h, grad, label }
    }

    /// Sharpness of the crack between pixels `l` and `r` (labels `la`, `lb`): ~1 for a real
    /// edge, small for a band boundary inside a gradient. `None` when not measurable.
    pub fn sharpness(&self, l: (i64, i64), r: (i64, i64), la: u16, lb: u16) -> Option<f64> {
        let inside =
            |p: (i64, i64)| p.0 >= 0 && p.1 >= 0 && (p.0 as usize) < self.w && (p.1 as usize) < self.h;
        if !inside(l) || !inside(r) || la == TRANSPARENT || lb == TRANSPARENT || la == lb {
            return None;
        }
        let contrast = self.label[la as usize].dist2(self.label[lb as usize]).sqrt();
        if contrast < 0.01 {
            return None;
        }
        let g = |p: (i64, i64)| self.grad[p.1 as usize * self.w + p.0 as usize];
        Some(f64::from(2.0 * g(l).max(g(r)) / contrast))
    }
}
