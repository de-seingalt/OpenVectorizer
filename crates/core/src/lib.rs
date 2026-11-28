//! `OpenVectorizer` engine.
//!
//! Pure computation: takes an RGBA buffer and [`Options`], returns an SVG document.
//! No file I/O, no threads, no platform APIs, so it compiles unchanged to native
//! and to `wasm32-unknown-unknown`.
//!
//! Pipeline (see `docs/ROADMAP.md`): [`palette`] cleans the colors, [`planar`] traces shared
//! boundaries and writes the SVG. The Photo preset still uses `VTracer` ([`baseline`]).
//! Each future stage (palette, planar map, sub-pixel edges, corners, primitives,
//! SVG writer) gets its own module and replaces part of the baseline.

pub mod baseline;
mod color;
mod denoise;
mod edges;
mod error;
mod fit;
pub mod guide;
mod image;
mod options;
pub mod palette;
pub mod planar;
mod repeat;
mod shape;

pub use error::Error;
pub use image::RgbaImage;
pub use options::{CornerEdit, CornerMode, Corners, Options, Preset, Stacking};
pub use palette::{ColorGroup, Palette, PaletteEdit, SourceColor};
pub use planar::CornerInfo;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Result of a vectorization.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Output {
    /// Complete SVG document (UTF-8).
    pub svg: String,
    pub stats: Stats,
    /// Source colors and output groups. `None` for the Photo preset.
    pub palette: Option<Palette>,
    /// Every corner of the result (empty for the Photo preset).
    pub corners: Vec<CornerInfo>,
    /// Guides found on the boundaries (empty for the Photo preset).
    pub guides: Vec<guide::Guide>,
}

/// Cheap statistics about the produced SVG, shown in the UI status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Stats {
    pub width: u32,
    pub height: u32,
    /// Number of `<path>` elements.
    pub path_count: u32,
    /// Size of the SVG document in bytes.
    pub bytes: u32,
    /// Corner candidates found, and how many of them are sharp.
    pub corners: u32,
    pub sharp_corners: u32,
}

/// Vectorize an RGBA image.
///
/// # Errors
/// Returns [`Error`] if the image is empty or the tracer fails.
pub fn vectorize(image: &RgbaImage, options: &Options) -> Result<Output, Error> {
    let options = options.validated()?;
    let (svg, palette, corners) = match options.preset {
        // Photos keep VTracer's own gradient layering for now.
        Preset::Photo => (baseline::trace(image, &options)?, None, planar::CornerStats::default()),
        Preset::Logo | Preset::Illustration => {
            // Compression artifacts are cleaned first: the colours and the sub-pixel edges are
            // measured on the cleaned image.
            let cleaned = denoise::denoise(image, options.denoise);
            let image = cleaned.as_ref().unwrap_or(image);
            let quantized = palette::quantize(image, &options);
            let (svg, corners) = planar::to_svg(&quantized, Some(image), &options, &fit_params(&options));
            (svg, Some(quantized.info), corners)
        }
    };
    let stats = Stats {
        width: image.width(),
        height: image.height(),
        path_count: svg.matches("<path").count() as u32,
        bytes: svg.len() as u32,
        corners: corners.total,
        sharp_corners: corners.sharp,
    };
    Ok(Output { svg, stats, palette, corners: corners.list, guides: corners.guides })
}

/// Curve fitting settings derived from the user options.
fn fit_params(o: &Options) -> fit::FitParams {
    // detail 0 → 1.25 px tolerance, 1 → 0.35 px.
    let tolerance = 0.35 + f64::from(1.0 - o.detail) * 0.9;
    // Scale 0..1 → up to 2 % of the size of a line, arc or shape.
    let relative = 0.02 * f64::from(o.scale);
    fit::FitParams { tolerance, arc_tol: fit::arc_tolerance(f64::from(o.geometry)), plain: false, relative }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_color_square() -> RgbaImage {
        // 32x32 white image with a 16x16 red square in the middle.
        let (w, h) = (32u32, 32u32);
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let inside = (8..24).contains(&x) && (8..24).contains(&y);
                data.extend_from_slice(if inside { &[220, 20, 20, 255] } else { &[255, 255, 255, 255] });
            }
        }
        RgbaImage::new(w, h, data).expect("valid buffer")
    }

    #[test]
    fn produces_svg_with_paths() {
        let out = vectorize(&two_color_square(), &Options::default()).expect("vectorize");
        assert!(out.svg.contains("<svg"));
        assert!(out.stats.path_count >= 1, "stats: {:?}", out.stats);
        assert_eq!((out.stats.width, out.stats.height), (32, 32));
    }

    #[test]
    fn band_boundaries_of_a_gradient_get_no_geometry() {
        // A red→yellow ramp across 160 px (quantized into vertical bands with straight
        // boundaries) and a sharp black square: only the square's sides are line guides.
        let (w, h) = (160u32, 80u32);
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let square = (20..50).contains(&x) && (25..55).contains(&y);
                let g = (60.0 + 180.0 * f64::from(x) / f64::from(w - 1)).round() as u8;
                let px: [u8; 4] = if square { [10, 10, 10, 255] } else { [230, g, 40, 255] };
                data.extend_from_slice(&px);
            }
        }
        let image = RgbaImage::new(w, h, data).expect("valid buffer");
        let options = Options { detail: 1.0, denoise: 0.0, max_colors: 8, ..Options::default() };
        let out = vectorize(&image, &options).expect("vectorize");
        assert!(out.svg.matches("<path").count() > 5, "bands expected: {}", out.stats.path_count);
        let lines: Vec<&guide::Guide> =
            out.guides.iter().filter(|g| g.kind == guide::GuideKind::Line).collect();
        assert!(!lines.is_empty(), "the square's sides are lines");
        for g in &lines {
            let on_square = (19.0..=51.0).contains(&g.x) && (24.0..=56.0).contains(&g.y);
            assert!(on_square, "line guide on a band boundary at ({}, {})", g.x, g.y);
        }
    }

    #[test]
    fn rejects_empty_image() {
        assert!(RgbaImage::new(0, 0, vec![]).is_err());
    }
}
