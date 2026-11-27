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
mod error;
mod fit;
mod image;
mod options;
pub mod palette;
pub mod planar;

pub use error::Error;
pub use image::RgbaImage;
pub use options::{Options, Preset, Stacking};
pub use palette::{ColorGroup, Palette, PaletteEdit, SourceColor};

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
}

/// Vectorize an RGBA image.
///
/// # Errors
/// Returns [`Error`] if the image is empty or the tracer fails.
pub fn vectorize(image: &RgbaImage, options: &Options) -> Result<Output, Error> {
    let options = options.validated()?;
    let (svg, palette) = match options.preset {
        // Photos keep VTracer's own gradient layering until M2.
        Preset::Photo => (baseline::trace(image, &options)?, None),
        Preset::Logo | Preset::Illustration => {
            let quantized = palette::quantize(image, &options);
            (
                planar::to_svg(&quantized, Some(image), options.stacking, &fit_params(&options)),
                Some(quantized.info),
            )
        }
    };
    let stats = Stats {
        width: image.width(),
        height: image.height(),
        path_count: svg.matches("<path").count() as u32,
        bytes: svg.len() as u32,
    };
    Ok(Output { svg, stats, palette })
}

/// Curve fitting settings derived from the user options.
fn fit_params(o: &Options) -> fit::FitParams {
    // detail 0 → 1.25 px tolerance, 1 → 0.35 px; smoothness 0 → corners from 25°, 1 → 135°.
    let tolerance = 0.35 + f64::from(1.0 - o.detail) * 0.9;
    let corner = 25.0 + f64::from(o.smoothness) * 110.0;
    fit::FitParams { tolerance, corner_cos: corner.to_radians().cos() }
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
    fn rejects_empty_image() {
        assert!(RgbaImage::new(0, 0, vec![]).is_err());
    }
}
