//! Quality evaluation for `OpenVectorizer`.
//!
//! The SVG is rasterized with resvg at the source resolution, both images are
//! composited on white, then compared pixel by pixel in Oklab. Together with
//! file-complexity counts this gives an objective score agents can optimize
//! (see `openvectorizer eval`).

use openvectorizer_core::RgbaImage;
use resvg::tiny_skia;
pub use resvg::usvg;
use serde::{Deserialize, Serialize};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot parse SVG: {0}")]
    Svg(String),
    #[error("cannot allocate {0}x{1} pixmap")]
    Pixmap(u32, u32),
    #[error(transparent)]
    Core(#[from] openvectorizer_core::Error),
}

/// Metrics for one (source, svg) pair. Lower `delta_e_*` is better, higher `psnr` is better,
/// lower `nodes`/`bytes` is better at equal fidelity.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    /// Mean Oklab distance x100 (≈ perceptual ΔE; < 1 invisible, > 5 obvious).
    pub delta_e_mean: f64,
    /// 95th percentile of the per-pixel ΔE (catches local defects: halos, gaps, lost details).
    pub delta_e_p95: f64,
    /// PSNR on sRGB, in dB.
    pub psnr: f64,
    pub paths: u32,
    /// Number of path commands (M, L, C, Q, A, Z…): proxy for editability.
    pub nodes: u32,
    pub bytes: u32,
}

/// Rasterize an SVG to RGBA at exactly `width` x `height`.
///
/// # Errors
/// Fails if the SVG cannot be parsed.
pub fn render_svg(svg: &str, width: u32, height: u32) -> Result<RgbaImage, Error> {
    render_svg_with(svg, width, height, &usvg::Options::default())
}

/// Like [`render_svg`] with custom parser options, e.g. a font database for SVGs
/// containing `<text>` (ground-truth typography). No system fonts are ever loaded,
/// so renders are identical on every machine.
///
/// # Errors
/// Fails if the SVG cannot be parsed.
pub fn render_svg_with(
    svg: &str,
    width: u32,
    height: u32,
    options: &usvg::Options,
) -> Result<RgbaImage, Error> {
    let tree = usvg::Tree::from_str(svg, options).map_err(|e| Error::Svg(e.to_string()))?;
    let mut pixmap = tiny_skia::Pixmap::new(width, height).ok_or(Error::Pixmap(width, height))?;
    let size = tree.size();
    let transform =
        tiny_skia::Transform::from_scale(width as f32 / size.width(), height as f32 / size.height());
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    let data = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect();
    Ok(RgbaImage::new(width, height, data)?)
}

/// Compare `svg` against `source`.
///
/// # Errors
/// Fails if the SVG cannot be rendered.
pub fn evaluate(source: &RgbaImage, svg: &str) -> Result<Metrics, Error> {
    let rendered = render_svg(svg, source.width(), source.height())?;
    let (mut des, sq_err) = per_pixel(source, &rendered);
    let n = des.len() as f64;
    let mse = sq_err / (n * 3.0);
    let psnr = if mse <= f64::EPSILON { 99.0 } else { 10.0 * (1.0 / mse).log10() };
    let delta_e_mean = des.iter().sum::<f64>() / n;
    des.sort_by(f64::total_cmp);
    let delta_e_p95 = des[((n * 0.95) as usize).min(des.len() - 1)];
    Ok(Metrics {
        delta_e_mean,
        delta_e_p95,
        psnr,
        paths: svg.matches("<path").count() as u32,
        nodes: count_path_commands(svg),
        bytes: svg.len() as u32,
    })
}

/// Error heatmap: black = identical, red/yellow = large ΔE (saturates at ΔE 20).
/// Meant to be *looked at* (by humans or agents) to locate defects.
///
/// # Errors
/// Fails if the SVG cannot be rendered.
pub fn heatmap(source: &RgbaImage, svg: &str) -> Result<RgbaImage, Error> {
    let rendered = render_svg(svg, source.width(), source.height())?;
    let (des, _) = per_pixel(source, &rendered);
    let data = des
        .iter()
        .flat_map(|d| {
            let t = (d / 20.0).clamp(0.0, 1.0);
            [(t * 2.0).min(1.0), (t * 2.0 - 1.0).max(0.0), 0.0]
                .map(|c| (c * 255.0) as u8)
                .into_iter()
                .chain([255])
        })
        .collect();
    Ok(RgbaImage::new(source.width(), source.height(), data)?)
}

/// Per-pixel ΔE (x100) and the summed squared sRGB error.
fn per_pixel(a: &RgbaImage, b: &RgbaImage) -> (Vec<f64>, f64) {
    let mut des = Vec::with_capacity((a.width() * a.height()) as usize);
    let mut sq_err = 0.0f64;
    for (pa, pb) in a.data().as_chunks::<4>().0.iter().zip(b.data().as_chunks::<4>().0) {
        let (ca, cb) = (over_white(pa), over_white(pb));
        for i in 0..3 {
            sq_err += (ca[i] - cb[i]).powi(2);
        }
        des.push(oklab_distance(ca, cb) * 100.0);
    }
    (des, sq_err)
}

/// sRGB (0..1) of an RGBA pixel composited over white.
fn over_white(p: &[u8]) -> [f64; 3] {
    let a = f64::from(p[3]) / 255.0;
    [0, 1, 2].map(|i| (f64::from(p[i]) / 255.0) * a + (1.0 - a))
}

fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Oklab (Björn Ottosson, 2020).
#[allow(clippy::many_single_char_names)] // standard Oklab notation
fn oklab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(srgb_to_linear);
    let l = (0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b).cbrt();
    let m = (0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b).cbrt();
    let s = (0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b).cbrt();
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

fn oklab_distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    let (la, lb) = (oklab(a), oklab(b));
    ((la[0] - lb[0]).powi(2) + (la[1] - lb[1]).powi(2) + (la[2] - lb[2]).powi(2)).sqrt()
}

/// Count path commands inside every `d="…"` attribute.
fn count_path_commands(svg: &str) -> u32 {
    let mut count = 0;
    let mut rest = svg;
    while let Some(start) = rest.find(" d=\"") {
        rest = &rest[start + 4..];
        let end = rest.find('"').unwrap_or(rest.len());
        count += rest[..end].chars().filter(|c| "MLHVCSQTAZmlhvcsqtaz".contains(*c)).count();
        rest = &rest[end..];
    }
    count as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="20">
        <rect width="20" height="20" fill="#fff"/><path d="M5 5 L15 5 L15 15 L5 15 Z" fill="#000"/></svg>"##;

    #[test]
    fn identical_render_is_perfect() {
        let src = render_svg(SQUARE, 20, 20).expect("render");
        let m = evaluate(&src, SQUARE).expect("eval");
        assert!(m.delta_e_mean < 1e-9 && m.psnr >= 99.0, "{m:?}");
        assert_eq!(m.nodes, 5);
    }

    #[test]
    fn different_image_scores_worse() {
        let white = RgbaImage::new(20, 20, vec![255; 20 * 20 * 4]).expect("img");
        let m = evaluate(&white, SQUARE).expect("eval");
        assert!(m.delta_e_mean > 10.0, "{m:?}");
    }
}
