//! Stages A and B of the custom algorithm (docs/ALGORITHME.md §4, §5.1): what the image is,
//! its noise, its Oklab planes smoothed at σ₀, and the pyramid for the large scales.
//!
//! Not yet used by [`crate::vectorize`]: the next phases build the edge detection on it. The
//! benchmark measures it (`openvectorizer analyze`).

use crate::RgbaImage;
use crate::raster::Plane;
use crate::raster::classify::{InputInfo, classify};
use crate::raster::gauss::{Kernel, smooth};
use crate::raster::noise::NoiseModel;
use crate::raster::oklab::{alpha_plane, encoded_planes, linear_planes, oklab_planes};
use crate::raster::pyramid::Pyramid;

/// Detection scale σ₀ (px) for ordinary and for small images (§4.1, §5.1).
pub const SIGMA0: f64 = 1.0;
pub const SIGMA0_SMALL: f64 = 0.7;

/// Everything the edge detection needs from the raster.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub info: InputInfo,
    pub noise: NoiseModel,
    pub sigma0: f64,
    /// L, a, b.
    pub oklab: [Plane<f32>; 3],
    /// L, a, b smoothed at σ₀.
    pub smoothed: [Plane<f32>; 3],
    /// Pyramids of L, a, b (level 0 is `oklab`, shared by copy).
    pub pyramids: [Pyramid; 3],
}

/// Duration of each stage, in the unit of the clock given to [`analyze`].
pub type Timings = Vec<(&'static str, f64)>;

/// Analyse `image`. `clock` returns the current time (ms); it is only read, so passing a
/// constant is fine when timings are not wanted (the engine has no clock of its own).
pub fn analyze(image: &RgbaImage, clock: &mut dyn FnMut() -> f64) -> (Analysis, Timings) {
    let mut timings = Vec::new();
    let mut last = clock();
    let mut lap = |name: &'static str, timings: &mut Timings, clock: &mut dyn FnMut() -> f64| {
        let now = clock();
        timings.push((name, now - last));
        last = now;
    };
    let info = classify(image);
    lap("classify", &mut timings, clock);
    let linear = linear_planes(image);
    let alpha = info.transparent.then(|| alpha_plane(image));
    let oklab = oklab_planes(&linear);
    lap("oklab", &mut timings, clock);
    drop(linear);
    let noise = NoiseModel::estimate(&encoded_planes(image), alpha.as_ref(), info.jpeg);
    lap("noise", &mut timings, clock);
    let sigma0 = if info.small { SIGMA0_SMALL } else { SIGMA0 };
    let kernel = Kernel::new(sigma0);
    let smoothed = [0, 1, 2].map(|c| smooth(&oklab[c], &kernel));
    lap("smooth", &mut timings, clock);
    let pyramids = [0, 1, 2].map(|c| Pyramid::new(oklab[c].clone()));
    lap("pyramid", &mut timings, clock);
    (Analysis { info, noise, sigma0, oklab, smoothed, pyramids }, timings)
}
