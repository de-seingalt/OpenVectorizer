//! What kind of image this is (docs/ALGORITHME.md §4.1): JPEG, pixel art, small, transparent.

use crate::RgbaImage;

/// Images whose smaller side is below this are "small" (finer smoothing, global noise).
pub const SMALL_SIDE: u32 = 128;
/// JPEG score above which the image is taken for a decoded JPEG (calibrated on the benchmark
/// with `openvectorizer analyze`).
pub const JPEG_SCORE: f64 = 1.1;
/// Share of the colour changes that must fall on the grid for pixel art.
pub const PIXEL_GRID_SHARE: f64 = 0.98;
pub const PIXEL_GRID_MAX: u32 = 32;
/// Pixel art has few colours.
pub const PIXEL_ART_MAX_COLORS: usize = 256;

/// The native grid of a pixel-art image: cell size and offset of the first full cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelGrid {
    pub cell: u32,
    pub offset_x: u32,
    pub offset_y: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InputInfo {
    pub width: u32,
    pub height: u32,
    /// Block-boundary discontinuity ratio (see `jpeg_score`), about 1 without block structure.
    pub jpeg_score: f64,
    pub jpeg: bool,
    pub pixel_art: Option<PixelGrid>,
    pub small: bool,
    /// Alpha is not constant.
    pub transparent: bool,
}

pub fn classify(image: &RgbaImage) -> InputInfo {
    let (w, h) = (image.width(), image.height());
    let px = image.data().as_chunks::<4>().0;
    let transparent = px.iter().any(|p| p[3] != px[0][3]);
    let pixel_art = pixel_grid(px, w, h);
    let jpeg_score = if pixel_art.is_some() { 1.0 } else { jpeg_score(px, w, h) };
    InputInfo {
        width: w,
        height: h,
        jpeg_score,
        jpeg: jpeg_score >= JPEG_SCORE,
        pixel_art,
        small: w.min(h) < SMALL_SIDE,
        transparent,
    }
}

/// Steps larger than this (luma ×8 units, i.e. 12 codes) are edges, left out of the test.
const JPEG_MAX_STEP: i32 = 96;

/// Integer luma (×8) of an RGBA pixel.
fn luma(p: [u8; 4]) -> i32 {
    2 * i32::from(p[0]) + 5 * i32::from(p[1]) + i32::from(p[2])
}

/// Mean step between neighbours across 8-px block boundaries over the mean step elsewhere,
/// counting only steps inside varying regions: the step and both its neighbours along the
/// line are small, and the neighbours are not zero. JPEG decoding leaves discontinuities at
/// block boundaries wherever the image varies (gradients, ringing around edges); an edge of the
/// drawing that happens to fall on the grid separates two flat areas (zero neighbour steps)
/// and is not counted. About 1 without block structure.
fn jpeg_score(px: &[[u8; 4]], w: u32, h: u32) -> f64 {
    if w < 16 || h < 16 {
        return 1.0;
    }
    let (w, h) = (w as usize, h as usize);
    // [boundary, inside] × (sum, count)
    let mut acc = [[0u64; 2]; 2];
    let mut line = |values: &mut dyn Iterator<Item = i32>| {
        let v: Vec<i32> = values.collect();
        for i in 1..v.len().saturating_sub(2) {
            let (a, d, b) = (v[i] - v[i - 1], v[i + 1] - v[i], v[i + 2] - v[i + 1]);
            let small = |s: i32| s.abs() <= JPEG_MAX_STEP;
            if small(a) && small(d) && small(b) && a != 0 && b != 0 {
                let slot = &mut acc[usize::from(i % 8 != 7)];
                slot[0] += u64::from(d.unsigned_abs());
                slot[1] += 1;
            }
        }
    };
    for y in 0..h {
        line(&mut (0..w).map(|x| luma(px[y * w + x])));
    }
    for x in 0..w {
        line(&mut (0..h).map(|y| luma(px[y * w + x])));
    }
    let mean = |s: [u64; 2]| if s[1] == 0 { 0.0 } else { s[0] as f64 / s[1] as f64 };
    let (on, off) = (mean(acc[0]), mean(acc[1]));
    if acc[0][1] < 64 || off <= 0.0 { 1.0 } else { on / off }
}

/// Largest cell size c ≥ 2 such that almost every colour change happens on a grid of period c,
/// in both directions, with few colours.
fn pixel_grid(px: &[[u8; 4]], w: u32, h: u32) -> Option<PixelGrid> {
    let (wu, hu) = (w as usize, h as usize);
    // Positions (between pixel i−1 and i) where the colour changes, per direction.
    let mut cols = vec![0u64; wu + 1];
    let mut rows = vec![0u64; hu + 1];
    for y in 0..hu {
        for x in 1..wu {
            if px[y * wu + x] != px[y * wu + x - 1] {
                cols[x] += 1;
            }
        }
    }
    for y in 1..hu {
        for x in 0..wu {
            if px[y * wu + x] != px[(y - 1) * wu + x] {
                rows[y] += 1;
            }
        }
    }
    let best = |changes: &[u64]| -> Option<(u32, u32)> {
        let total: u64 = changes.iter().sum();
        if total < 32 {
            return None;
        }
        let mut found = None;
        for c in 2..=PIXEL_GRID_MAX.min((changes.len() / 2) as u32) {
            let mut by_phase = vec![0u64; c as usize];
            for (i, &n) in changes.iter().enumerate() {
                by_phase[i % c as usize] += n;
            }
            let (phase, &top) =
                by_phase.iter().enumerate().max_by_key(|&(i, &n)| (n, std::cmp::Reverse(i)))?;
            if top as f64 >= PIXEL_GRID_SHARE * total as f64 {
                found = Some((c, phase as u32));
            }
        }
        found
    };
    let ((cx, ox), (cy, oy)) = (best(&cols)?, best(&rows)?);
    if cx != cy {
        return None;
    }
    let mut colors: Vec<[u8; 4]> = px.iter().step_by(cx as usize).copied().collect();
    colors.sort_unstable();
    colors.dedup();
    (colors.len() <= PIXEL_ART_MAX_COLORS).then_some(PixelGrid { cell: cx, offset_x: ox, offset_y: oy })
}
