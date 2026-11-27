//! M0 baseline: delegate tracing to `VTracer` (MIT/Apache-2.0).
//!
//! This module is a stand-in. It will be replaced stage by stage by our own
//! pipeline; the evaluation harness (`crates/eval`) measures each replacement
//! against it.

use crate::{Error, Options, Preset, RgbaImage, Stacking};

/// Trace `image` to an SVG string.
///
/// # Errors
/// Propagates `VTracer` failures as [`Error::Trace`].
pub fn trace(image: &RgbaImage, options: &Options) -> Result<String, Error> {
    run(image, to_vtracer_config(options))
}

/// Trace an image whose colors were already quantized by [`crate::palette`]:
/// `VTracer` must keep every color as is and not filter specks again.
///
/// # Errors
/// Propagates `VTracer` failures as [`Error::Trace`].
pub fn trace_flat(image: &RgbaImage, options: &Options) -> Result<String, Error> {
    let mut c = to_vtracer_config(options);
    c.color_precision = 8;
    c.layer_difference = 1;
    c.filter_speckle = 0;
    run(image, c)
}

fn run(image: &RgbaImage, config: vtracer::Config) -> Result<String, Error> {
    let img = vtracer::ColorImage {
        pixels: image.data().to_vec(),
        width: image.width() as usize,
        height: image.height() as usize,
    };
    let svg = vtracer::convert(img, config).map_err(Error::Trace)?;
    Ok(add_view_box(&svg.to_string(), image.width(), image.height()))
}

/// `VTracer` only writes width/height; a viewBox is required for the SVG to scale
/// (UI zoom, export at another size).
fn add_view_box(svg: &str, width: u32, height: u32) -> String {
    match svg.find("<svg ") {
        Some(i) if !svg[i..].split('>').next().unwrap_or("").contains("viewBox") => {
            format!("{}<svg viewBox=\"0 0 {width} {height}\" {}", &svg[..i], &svg[i + 5..])
        }
        _ => svg.to_owned(),
    }
}

fn to_vtracer_config(o: &Options) -> vtracer::Config {
    let base = match o.preset {
        Preset::Logo | Preset::Illustration => vtracer::Preset::Poster,
        Preset::Photo => vtracer::Preset::Photo,
    };
    let mut c = vtracer::Config::from_preset(base);
    c.hierarchical = match o.stacking {
        Stacking::Stacked => vtracer::Hierarchical::Stacked,
        Stacking::Cutout => vtracer::Hierarchical::Cutout,
    };
    let detail = f64::from(o.detail);
    // detail -> more color bits and smaller gradient steps between layers
    c.color_precision = 4 + (detail * 4.0).round() as i32; // 4..=8
    c.layer_difference = (48.0 - detail * 40.0).round() as i32; // 48..=8
    c.filter_speckle = o.min_area_px as usize;
    // smoothness -> corner angle threshold (degrees): higher = fewer sharp corners
    c.corner_threshold = (30.0 + f64::from(o.smoothness) * 120.0).round() as i32; // 30..=150
    c
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds_view_box_once() {
        let out = super::add_view_box(r#"<?xml?><svg width="3" height="2"></svg>"#, 3, 2);
        assert_eq!(out, r#"<?xml?><svg viewBox="0 0 3 2" width="3" height="2"></svg>"#);
        assert_eq!(super::add_view_box(&out, 3, 2), out);
    }
}
