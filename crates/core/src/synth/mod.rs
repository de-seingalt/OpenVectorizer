//! Synthetic scenes with an exact ground truth (docs/ALGORITHME.md §13.1).
//!
//! A [`Scene`] lists shapes with their exact geometry ([`Geom`]) and fill. It is rendered with
//! exact area coverage, shape by shape like an SVG renderer, then optionally degraded (blur,
//! noise, upscaling) by [`degrade`]. The scene itself is the ground truth: [`Geom::features`]
//! lists the corners, segments, circles and ellipses a perfect vectorizer would find.
//!
//! Compiled for tests and with the `synth` feature (calibration tools, benchmarks).

pub mod degrade;
pub mod geom;
pub mod random;

pub use geom::{Cubic, Feature, Geom, Pt};

use crate::RgbaImage;
use crate::raster::{Coverage, Plane};

/// Flattening tolerance of curved outlines (px): far below any accuracy target.
pub const FLATTEN_TOLERANCE: f64 = 1e-4;

/// An sRGB colour, gamma-encoded, components in [0, 1].
pub type Rgb = [f64; 3];

/// How a shape is painted.
#[derive(Debug, Clone, PartialEq)]
pub enum Fill {
    Solid(Rgb),
    /// Linear gradient from `c0` at `from` to `c1` at `to` (interpolated in sRGB, like SVG).
    Linear {
        from: Pt,
        to: Pt,
        c0: Rgb,
        c1: Rgb,
    },
}

impl Fill {
    fn at(&self, p: Pt) -> Rgb {
        match self {
            Fill::Solid(c) => *c,
            Fill::Linear { from, to, c0, c1 } => {
                let d = (to.0 - from.0, to.1 - from.1);
                let t =
                    (((p.0 - from.0) * d.0 + (p.1 - from.1) * d.1) / (d.0 * d.0 + d.1 * d.1)).clamp(0.0, 1.0);
                [0, 1, 2].map(|i| c0[i] + (c1[i] - c0[i]) * t)
            }
        }
    }
}

/// Colour space in which anti-aliased edges are blended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Blend {
    /// Gamma-encoded sRGB, like browsers and most SVG renderers.
    #[default]
    Srgb,
    /// Linear light, like an optical system.
    Linear,
}

/// A painted shape.
#[derive(Debug, Clone, PartialEq)]
pub struct Shape {
    pub geom: Geom,
    pub fill: Fill,
    /// In [0, 1].
    pub opacity: f64,
}

impl Shape {
    pub fn solid(geom: Geom, color: Rgb) -> Self {
        Shape { geom, fill: Fill::Solid(color), opacity: 1.0 }
    }
}

/// Shapes painted in order over a background.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    /// Background colour and alpha (alpha 0 = transparent).
    pub background: Rgb,
    pub background_alpha: f64,
    pub shapes: Vec<Shape>,
    pub blend: Blend,
}

impl Scene {
    pub fn new(width: u32, height: u32, background: Rgb) -> Self {
        Scene { width, height, background, background_alpha: 1.0, shapes: Vec::new(), blend: Blend::Srgb }
    }

    #[must_use]
    pub fn with(mut self, shape: Shape) -> Self {
        self.shapes.push(shape);
        self
    }

    /// Render with exact coverage. Planes: R, G, B (gamma-encoded sRGB) and alpha, in [0, 1].
    pub fn render(&self) -> [Plane<f64>; 4] {
        let (w, h) = (self.width, self.height);
        let to_space = |c: f64| if self.blend == Blend::Linear { srgb_decode(c) } else { c };
        let mut rgb = [0, 1, 2].map(|i| Plane::new(w, h, to_space(self.background[i])));
        let mut alpha = Plane::new(w, h, self.background_alpha);
        for shape in &self.shapes {
            let mut cov = Coverage::new(w, h);
            for c in shape.geom.contours(FLATTEN_TOLERANCE) {
                cov.contour(&c);
            }
            let cov = cov.finish();
            for y in 0..h {
                for x in 0..w {
                    let k = cov.get(x, y) * shape.opacity;
                    if k == 0.0 {
                        continue;
                    }
                    let fill = shape.fill.at((f64::from(x) + 0.5, f64::from(y) + 0.5));
                    // Source-over with non-premultiplied output.
                    let a0 = alpha.get(x, y);
                    let a = k + a0 * (1.0 - k);
                    for (i, plane) in rgb.iter_mut().enumerate() {
                        let mixed = (to_space(fill[i]) * k + plane.get(x, y) * a0 * (1.0 - k)) / a;
                        plane.set(x, y, mixed);
                    }
                    alpha.set(x, y, a);
                }
            }
        }
        if self.blend == Blend::Linear {
            for plane in &mut rgb {
                *plane = plane.map(srgb_encode);
            }
        }
        let [r, g, b] = rgb;
        [r, g, b, alpha]
    }

    /// Render and quantize to 8 bits.
    pub fn image(&self) -> RgbaImage {
        quantize(&self.render())
    }

    /// Ground truth: the features of every shape, in painting order.
    pub fn features(&self) -> Vec<Feature> {
        self.shapes.iter().flat_map(|s| s.geom.features()).collect()
    }
}

/// sRGB transfer function, decoding (exact formula, deterministic `pow`).
pub fn srgb_decode(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { crate::num::math::pow((c + 0.055) / 1.055, 2.4) }
}

/// sRGB transfer function, encoding.
pub fn srgb_encode(v: f64) -> f64 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * crate::num::math::pow(v, 1.0 / 2.4) - 0.055 }
}

/// 8-bit RGBA image from four [0, 1] planes (rounded to nearest).
///
/// # Panics
/// If the planes are larger than [`RgbaImage`] accepts.
pub fn quantize(planes: &[Plane<f64>; 4]) -> RgbaImage {
    let (w, h) = (planes[0].width(), planes[0].height());
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h {
        for x in 0..w {
            for p in planes {
                data.push((p.get(x, y).clamp(0.0, 1.0) * 255.0).round() as u8);
            }
        }
    }
    RgbaImage::new(w, h, data).expect("scene size is valid")
}

#[cfg(test)]
mod tests;
