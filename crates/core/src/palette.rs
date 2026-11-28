//! M1 step 1: palette extraction and pixel labelling.
//!
//! Turns a noisy, blurred or JPEG-compressed image into a clean label map with a
//! small palette, before tracing:
//! 1. **Flat pixels** (low local gradient after a 3×3 box blur) represent the real
//!    colors; edge pixels (anti-aliasing, blur, JPEG ringing) only weigh a little.
//! 2. Weighted k-means in Oklab over a 15-bit color histogram, then close clusters
//!    are merged and **blend clusters** (colors lying between two others and made
//!    mostly of edge pixels) are removed.
//! 3. Flat pixels take the nearest palette color; edge pixels choose among the
//!    colors of nearby flat pixels, so a black/white transition never becomes gray.
//!    A thin line (no flat pixel of its own) keeps its color when it is much closer.
//! 4. Connected components smaller than `min_area_px` join their main neighbor.
//!
//! Two levels of colors: **sources** are fine color clusters (at most [`MAX_SOURCES`]),
//! stable for a given image and detail; **groups** are the output colors. Each source belongs
//! to one group: automatically (merge threshold from `detail`), limited by `max_colors`, or as
//! edited by the user ([`PaletteEdit`]). Pixels are labelled with sources, then mapped to groups.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::color::{Lab, oklab_to_srgb, srgb_to_oklab};
use crate::num::sq;
use crate::{Error, Options, RgbaImage};

/// Pixel label for transparent pixels.
pub const TRANSPARENT: u16 = u16::MAX;

/// Max channel difference (0..=255) between neighboring blurred pixels for a pixel to be flat.
const FLAT_THRESHOLD: u8 = 14;
/// Weight of a non-flat pixel in the k-means (flat pixels weigh 1).
const EDGE_WEIGHT: f32 = 0.2;
/// Radius of the window searched for flat neighbors when labelling an edge pixel.
const EDGE_RADIUS: i64 = 3;
const KMEANS_ITERATIONS: usize = 12;
/// Maximum number of source colors offered to the user.
pub const MAX_SOURCES: usize = 24;
/// Sources closer than this (Oklab units, ×100 ≈ ΔE) are the same color.
const SOURCE_MERGE: f32 = 0.035;
/// An edge-only cluster this close to a heavier color is its anti-aliasing fringe.
const FRINGE_DISTANCE: f32 = 0.08;

/// Colors of a result: the fine source colors and the output groups they belong to.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Palette {
    /// Fingerprint of `sources`; a [`PaletteEdit`] applies only when its key matches.
    pub key: u32,
    /// Source colors, most frequent first.
    pub sources: Vec<SourceColor>,
    /// Output colors, in order of their first source.
    pub groups: Vec<ColorGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SourceColor {
    /// `#rrggbb`
    pub color: String,
    /// Pixels of the image closest to this color.
    pub pixels: u32,
    /// Index into [`Palette::groups`].
    pub group: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ColorGroup {
    /// `#rrggbb` used in the SVG.
    pub color: String,
    /// Pixels drawn with this color.
    pub pixels: u32,
    /// The color was chosen by the user.
    pub custom: bool,
}

/// User changes to the automatic grouping (made in the palette editor).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PaletteEdit {
    /// [`Palette::key`] of the palette that was edited.
    pub key: u32,
    /// Group id of each source (any numbers; groups are renumbered in the result).
    pub groups: Vec<u32>,
    /// Optional custom `#rrggbb` color per group id.
    pub colors: Vec<Option<String>>,
}

impl PaletteEdit {
    /// # Errors
    /// Returns [`Error::InvalidOption`] for a malformed color.
    pub fn validate(&self) -> Result<(), Error> {
        for c in self.colors.iter().flatten() {
            parse_hex(c).ok_or_else(|| Error::InvalidOption(format!("invalid color {c:?}")))?;
        }
        Ok(())
    }
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

fn hex([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Result of [`quantize`].
#[derive(Debug, Clone)]
pub struct Quantized {
    pub width: u32,
    pub height: u32,
    /// Output colors (sRGB), one per group.
    pub palette: Vec<[u8; 3]>,
    /// One entry per pixel: index into `palette`, or [`TRANSPARENT`].
    pub labels: Vec<u16>,
    /// Source colors of each group as found in the image (ignores custom colors): used to
    /// measure how much of each color an anti-aliased pixel contains.
    pub model: Vec<Vec<[u8; 3]>>,
    /// Sources and groups, for the palette editor.
    pub info: Palette,
}

impl Quantized {
    /// Render the label map as an RGBA image (transparent pixels stay transparent).
    ///
    /// # Panics
    /// Never for a [`Quantized`] built by [`quantize`] (same size as its source).
    pub fn to_image(&self) -> RgbaImage {
        let mut data = Vec::with_capacity(self.labels.len() * 4);
        for &l in &self.labels {
            if l == TRANSPARENT {
                data.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                let [r, g, b] = self.palette[l as usize];
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
        RgbaImage::new(self.width, self.height, data).expect("same dimensions as the source image")
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Cluster {
    center: Lab,
    /// k-means weight (flat pixels count 1, edge pixels [`EDGE_WEIGHT`]).
    weight: f32,
    pixels: u32,
    flat: u32,
}

/// Extract a palette and label every pixel.
pub fn quantize(image: &RgbaImage, options: &Options) -> Quantized {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgba = image.data();
    let opaque: Vec<bool> = rgba.as_chunks::<4>().0.iter().map(|p| p[3] >= 128).collect();
    let flat = flat_mask(rgba, &opaque, w, h);

    let points = histogram(rgba, &opaque, &flat);
    let detail = options.detail;
    let k0 = 8 + (detail * 40.0).round() as usize;
    let merge_threshold = (12.0 - 8.0 * detail) / 100.0;
    let opaque_count = opaque.iter().filter(|&&o| o).count();
    let min_pixels = (options.min_area_px as usize).max(opaque_count / 10_000).max(1) as u32;

    // Sources: fine clusters, without blends, at most MAX_SOURCES.
    let mut clusters = seed(&points, k0);
    lloyd(&points, &mut clusters, KMEANS_ITERATIONS);
    merge_close(&mut clusters, SOURCE_MERGE, 1);
    lloyd(&points, &mut clusters, 3);
    remove_spurious(&mut clusters, &points, min_pixels);
    lloyd(&points, &mut clusters, 3);
    clusters.retain(|c| c.pixels > 0);
    if clusters.len() > MAX_SOURCES {
        merge_close(&mut clusters, f32::INFINITY, MAX_SOURCES);
        lloyd(&points, &mut clusters, 2);
        clusters.retain(|c| c.pixels > 0);
    }
    clusters.sort_by_key(|c| std::cmp::Reverse(c.pixels));
    let sources = clusters;
    let key = fingerprint(&sources);

    // Groups: edited by the user, limited to max_colors, or automatic.
    let (groups, custom) = match &options.palette_edit {
        Some(edit) if edit.key == key && edit.groups.len() == sources.len() => {
            let (groups, centers) = renumber(&edit.groups, &sources);
            let custom: Vec<Option<[u8; 3]>> = edit_colors(edit, &groups, &centers);
            ((groups, centers), custom)
        }
        _ => {
            let max = if options.max_colors >= 2 { options.max_colors as usize } else { usize::MAX };
            let g = agglomerate(&sources, merge_threshold, max);
            let n = g.1.len();
            (g, vec![None; n])
        }
    };
    let (group_of, group_centers) = groups;

    let source_centers: Vec<Lab> = sources.iter().map(|c| c.center).collect();
    let mut labels = assign(rgba, &opaque, &flat, w, h, &source_centers);
    for l in &mut labels {
        if *l != TRANSPARENT {
            *l = group_of[*l as usize] as u16;
        }
    }
    if options.min_area_px > 0 {
        remove_specks(&mut labels, w, h, options.min_area_px as usize);
    }
    let palette: Vec<[u8; 3]> = group_centers
        .iter()
        .zip(&custom)
        .map(|(&c, custom)| custom.unwrap_or_else(|| oklab_to_srgb(c)))
        .collect();
    let mut group_pixels = vec![0u32; palette.len()];
    for &l in &labels {
        if l != TRANSPARENT {
            group_pixels[l as usize] += 1;
        }
    }
    let info = Palette {
        key,
        sources: sources
            .iter()
            .zip(&group_of)
            .map(|(c, &g)| SourceColor { color: hex(oklab_to_srgb(c.center)), pixels: c.pixels, group: g })
            .collect(),
        groups: palette
            .iter()
            .zip(&group_pixels)
            .zip(&custom)
            .map(|((&c, &pixels), custom)| ColorGroup { color: hex(c), pixels, custom: custom.is_some() })
            .collect(),
    };
    let mut model = vec![Vec::new(); group_centers.len()];
    for (c, &g) in sources.iter().zip(&group_of) {
        model[g as usize].push(oklab_to_srgb(c.center));
    }
    Quantized { width: image.width(), height: image.height(), palette, labels, model, info }
}

/// FNV-1a hash of the source colors.
fn fingerprint(sources: &[Cluster]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for c in sources {
        for b in oklab_to_srgb(c.center) {
            h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
        }
    }
    h
}

type Grouping = (Vec<u32>, Vec<Lab>);

/// Renumber user group ids in order of first appearance; group colors are the weighted means.
fn renumber(ids: &[u32], sources: &[Cluster]) -> (Vec<u32>, Vec<Lab>) {
    let mut map: Vec<(u32, u32)> = Vec::new();
    let groups: Vec<u32> = ids
        .iter()
        .map(|&id| {
            if let Some(&(_, g)) = map.iter().find(|(k, _)| *k == id) {
                g
            } else {
                let g = map.len() as u32;
                map.push((id, g));
                g
            }
        })
        .collect();
    let mut sum = vec![(Lab::default(), 0.0f32); map.len()];
    for (s, &g) in sources.iter().zip(&groups) {
        let e = &mut sum[g as usize];
        e.0 = e.0.add(s.center.scale(s.weight));
        e.1 += s.weight;
    }
    let centers = sum.into_iter().map(|(c, w)| c.scale(1.0 / w.max(1e-6))).collect();
    (groups, centers)
}

/// Custom colors of the edit, indexed by the renumbered groups.
fn edit_colors(edit: &PaletteEdit, groups: &[u32], centers: &[Lab]) -> Vec<Option<[u8; 3]>> {
    let mut out = vec![None; centers.len()];
    for (&id, &g) in edit.groups.iter().zip(groups) {
        if let Some(Some(c)) = edit.colors.get(id as usize) {
            out[g as usize] = parse_hex(c);
        }
    }
    out
}

/// Agglomerative grouping of the sources: merge the closest groups while they are closer than
/// `threshold` or more than `max` groups remain. Close colors average (noise, shading); a merge
/// forced by `max` keeps the dominant color, so every output color exists in the image.
fn agglomerate(sources: &[Cluster], threshold: f32, max: usize) -> Grouping {
    let mut groups: Vec<(Cluster, Vec<usize>)> =
        sources.iter().enumerate().map(|(i, &c)| (c, vec![i])).collect();
    while groups.len() > 1 {
        let mut best = (0, 0, f32::MAX);
        for i in 0..groups.len() {
            for j in i + 1..groups.len() {
                let d = groups[i].0.center.dist2(groups[j].0.center);
                if d < best.2 {
                    best = (i, j, d);
                }
            }
        }
        let (i, j, d) = best;
        let close = d < threshold * threshold;
        if !close && groups.len() <= max {
            break;
        }
        let (b, members) = groups.swap_remove(j);
        let a = &mut groups[i];
        let weight = a.0.weight + b.weight;
        a.0.center = if close {
            a.0.center.scale(a.0.weight / weight).add(b.center.scale(b.weight / weight))
        } else if a.0.weight >= b.weight {
            a.0.center
        } else {
            b.center
        };
        a.0.weight = weight;
        a.0.pixels += b.pixels;
        a.1.extend(members);
    }
    // Number groups in order of their first (most frequent) source.
    groups.sort_by_key(|g| g.1.iter().copied().min());
    let mut group_of = vec![0u32; sources.len()];
    for (g, (_, members)) in groups.iter().enumerate() {
        for &m in members {
            group_of[m] = g as u32;
        }
    }
    (group_of, groups.iter().map(|g| g.0.center).collect())
}

fn rgb_at(rgba: &[u8], i: usize) -> [u8; 3] {
    [rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]]
}

/// A pixel is flat when its 3×3-blurred color barely differs from its 4 neighbors'.
fn flat_mask(rgba: &[u8], opaque: &[bool], w: usize, h: usize) -> Vec<bool> {
    let mut blurred = vec![[0u8; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let (mut sum, mut n) = ([0u32; 3], 0u32);
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = yy * w + xx;
                    if opaque[j] {
                        let c = rgb_at(rgba, j);
                        for k in 0..3 {
                            sum[k] += u32::from(c[k]);
                        }
                        n += 1;
                    }
                }
            }
            if n > 0 {
                blurred[y * w + x] = sum.map(|s| ((s + n / 2) / n) as u8);
            }
        }
    }
    let diff = |a: [u8; 3], b: [u8; 3]| (0..3).map(|k| a[k].abs_diff(b[k])).max().unwrap_or(0);
    let mut flat = vec![false; w * h];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !opaque[i] {
                continue;
            }
            let c = blurred[i];
            let mut max = 0;
            let mut visit = |j: usize| {
                if opaque[j] {
                    max = max.max(diff(c, blurred[j]));
                } else {
                    max = u8::MAX; // next to transparency: an edge
                }
            };
            if x > 0 {
                visit(i - 1);
            }
            if x + 1 < w {
                visit(i + 1);
            }
            if y > 0 {
                visit(i - w);
            }
            if y + 1 < h {
                visit(i + w);
            }
            flat[i] = max < FLAT_THRESHOLD;
        }
    }
    flat
}

/// 15-bit color histogram; each non-empty bin becomes a weighted point at its mean Oklab color.
fn histogram(rgba: &[u8], opaque: &[bool], flat: &[bool]) -> Vec<Cluster> {
    let mut bins = vec![Cluster::default(); 1 << 15];
    for (i, &o) in opaque.iter().enumerate() {
        if !o {
            continue;
        }
        let c = rgb_at(rgba, i);
        let key = (usize::from(c[0] >> 3) << 10) | (usize::from(c[1] >> 3) << 5) | usize::from(c[2] >> 3);
        let bin = &mut bins[key];
        bin.center = bin.center.add(srgb_to_oklab(c));
        bin.pixels += 1;
        if flat[i] {
            bin.flat += 1;
        }
    }
    bins.into_iter()
        .filter(|b| b.pixels > 0)
        .map(|b| Cluster {
            center: b.center.scale(1.0 / b.pixels as f32),
            weight: b.flat as f32 + EDGE_WEIGHT * (b.pixels - b.flat) as f32,
            ..b
        })
        .collect()
}

/// Deterministic k-means++-style seeding: heaviest point first, then the point
/// maximizing `weight × distance²` to the chosen seeds.
fn seed(points: &[Cluster], k: usize) -> Vec<Cluster> {
    let Some(first) = points.iter().max_by(|a, b| a.weight.total_cmp(&b.weight)) else {
        return Vec::new();
    };
    let mut seeds = vec![*first];
    let mut d2: Vec<f32> = points.iter().map(|p| p.center.dist2(first.center)).collect();
    while seeds.len() < k {
        let (best, score) = points
            .iter()
            .zip(&d2)
            .map(|(p, &d)| p.weight * d)
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .unwrap_or((0, 0.0));
        if score < 1e-6 {
            break; // every point is already close to a seed
        }
        let c = points[best].center;
        seeds.push(points[best]);
        for (d, p) in d2.iter_mut().zip(points) {
            *d = d.min(p.center.dist2(c));
        }
    }
    seeds
}

fn nearest(centers: impl Iterator<Item = Lab>, c: Lab) -> (usize, f32) {
    centers.map(|k| k.dist2(c)).enumerate().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, f32::MAX))
}

fn lloyd(points: &[Cluster], clusters: &mut [Cluster], iterations: usize) {
    for _ in 0..iterations {
        let mut acc = vec![Cluster::default(); clusters.len()];
        for p in points {
            let (k, _) = nearest(clusters.iter().map(|c| c.center), p.center);
            let a = &mut acc[k];
            a.center = a.center.add(p.center.scale(p.weight));
            a.weight += p.weight;
            a.pixels += p.pixels;
            a.flat += p.flat;
        }
        for (c, a) in clusters.iter_mut().zip(acc) {
            if a.weight > 0.0 {
                *c = Cluster { center: a.center.scale(1.0 / a.weight), ..a };
            } else {
                *c = Cluster { center: c.center, ..Cluster::default() };
            }
        }
    }
}

/// Merge the closest pair of clusters while it is closer than `threshold` (Oklab units)
/// and more than `min_count` clusters remain.
fn merge_close(clusters: &mut Vec<Cluster>, threshold: f32, min_count: usize) {
    clusters.retain(|c| c.weight > 0.0);
    while clusters.len() > min_count.max(1) {
        let mut best = (0, 0, f32::MAX);
        for i in 0..clusters.len() {
            for j in i + 1..clusters.len() {
                let d = clusters[i].center.dist2(clusters[j].center);
                if d < best.2 {
                    best = (i, j, d);
                }
            }
        }
        let (i, j, d) = best;
        if d >= threshold * threshold && threshold.is_finite() {
            return;
        }
        let (a, b) = (clusters[i], clusters[j]);
        let weight = a.weight + b.weight;
        let center = a.center.scale(a.weight / weight).add(b.center.scale(b.weight / weight));
        clusters[i] = Cluster { center, weight, pixels: a.pixels + b.pixels, flat: a.flat + b.flat };
        clusters.swap_remove(j);
    }
}

/// Drop clusters that are too small, or that are anti-aliasing blends of real colors.
///
/// A cluster made mostly of edge pixels is a blend when its center lies between two heavier
/// kept clusters, and a fringe when it lies close to one (anti-aliasing near one end). k-means can also pool blends of several color pairs into one small cluster
/// whose center lies between none: such a cluster (no flat pixel, under 1 % of the image) is
/// dropped when most of its pixels lie between two heavier kept clusters.
fn remove_spurious(clusters: &mut Vec<Cluster>, points: &[Cluster], min_pixels: u32) {
    clusters.sort_by(|a, b| b.weight.total_cmp(&a.weight));
    let total: u32 = clusters.iter().map(|c| c.pixels).sum();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); clusters.len()];
    for (i, p) in points.iter().enumerate() {
        members[nearest(clusters.iter().map(|c| c.center), p.center).0].push(i);
    }
    let mut keep = vec![true; clusters.len()];
    for c in 1..clusters.len() {
        let cl = clusters[c];
        if cl.pixels < min_pixels {
            keep[c] = false;
            continue;
        }
        if cl.flat * 2 >= cl.pixels {
            continue;
        }
        let heavier: Vec<Lab> = (0..c).filter(|&a| keep[a]).map(|a| clusters[a].center).collect();
        let between = |color: Lab| {
            heavier.iter().enumerate().any(|(i, &a)| heavier[i + 1..].iter().any(|&b| is_blend(a, b, color)))
        };
        let fringe = heavier.iter().any(|&a| a.dist2(cl.center) < FRINGE_DISTANCE * FRINGE_DISTANCE);
        if fringe || between(cl.center) {
            keep[c] = false;
        } else if cl.flat * 10 < cl.pixels && cl.pixels * 100 < total {
            let blended: u32 =
                members[c].iter().filter(|&&i| between(points[i].center)).map(|&i| points[i].pixels).sum();
            keep[c] = blended * 2 < cl.pixels;
        }
    }
    let mut it = keep.iter();
    clusters.retain(|_| *it.next().unwrap_or(&true));
}

/// `c` lies on the segment between `a` and `b`, away from both ends. Anti-aliasing
/// and blur mix colors linearly in sRGB (not in Oklab), so the test is done there.
fn is_blend(a: Lab, b: Lab, c: Lab) -> bool {
    let rgb = |l: Lab| oklab_to_srgb(l).map(f32::from);
    let (a, b, c) = (rgb(a), rgb(b), rgb(c));
    let ab: [f32; 3] = std::array::from_fn(|k| b[k] - a[k]);
    let ac: [f32; 3] = std::array::from_fn(|k| c[k] - a[k]);
    let len2: f32 = ab.iter().map(|v| v * v).sum();
    if len2 < 1.0 {
        return false;
    }
    let t = ab.iter().zip(&ac).map(|(x, y)| x * y).sum::<f32>() / len2;
    if !(0.1..=0.9).contains(&t) {
        return false;
    }
    let off2: f32 = ab.iter().zip(&ac).map(|(x, y)| sq(y - t * x)).sum();
    let tolerance = 12f32.max(0.06 * len2.sqrt());
    off2 < tolerance * tolerance
}

fn assign(rgba: &[u8], opaque: &[bool], flat: &[bool], w: usize, h: usize, centers: &[Lab]) -> Vec<u16> {
    let mut labels = vec![TRANSPARENT; w * h];
    if centers.is_empty() {
        return labels;
    }
    let labs: Vec<Lab> =
        (0..w * h).map(|i| if opaque[i] { srgb_to_oklab(rgb_at(rgba, i)) } else { Lab::default() }).collect();
    for i in 0..w * h {
        if opaque[i] && flat[i] {
            labels[i] = nearest(centers.iter().copied(), labs[i]).0 as u16;
        }
    }
    let flat_labels = labels.clone();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            if !opaque[i] || flat[i] {
                continue;
            }
            let c = labs[i];
            let (global, dg) = nearest(centers.iter().copied(), c);
            let mut local: Option<(u16, f32)> = None;
            for yy in (y as i64 - EDGE_RADIUS).max(0)..=(y as i64 + EDGE_RADIUS).min(h as i64 - 1) {
                for xx in (x as i64 - EDGE_RADIUS).max(0)..=(x as i64 + EDGE_RADIUS).min(w as i64 - 1) {
                    let l = flat_labels[yy as usize * w + xx as usize];
                    if l != TRANSPARENT {
                        let d = centers[l as usize].dist2(c);
                        if local.is_none_or(|(_, best)| d < best) {
                            local = Some((l, d));
                        }
                    }
                }
            }
            // Prefer a color present nearby, unless a palette color is much closer (thin lines).
            labels[i] = match local {
                Some((l, dl)) if dg >= 0.25 * dl => l,
                _ => global as u16,
            };
        }
    }
    labels
}

/// Merge 4-connected components smaller than `min_area` into the neighbor label they touch most.
fn remove_specks(labels: &mut [u16], w: usize, h: usize, min_area: usize) {
    let mut seen = vec![false; w * h];
    let mut stack = Vec::new();
    let mut component = Vec::new();
    for start in 0..w * h {
        if seen[start] || labels[start] == TRANSPARENT {
            continue;
        }
        let label = labels[start];
        component.clear();
        stack.push(start);
        seen[start] = true;
        let mut contacts: Vec<(u16, u32)> = Vec::new();
        while let Some(i) = stack.pop() {
            component.push(i);
            let (x, y) = (i % w, i / w);
            let neighbors = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in neighbors.into_iter().flatten() {
                let l = labels[j];
                if l == label {
                    if !seen[j] {
                        seen[j] = true;
                        stack.push(j);
                    }
                } else if l != TRANSPARENT && component.len() < min_area {
                    match contacts.iter_mut().find(|(k, _)| *k == l) {
                        Some((_, n)) => *n += 1,
                        None => contacts.push((l, 1)),
                    }
                }
            }
        }
        if component.len() < min_area
            && let Some(&(target, _)) = contacts.iter().max_by_key(|(_, n)| *n)
        {
            for &i in &component {
                labels[i] = target;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, mut f: impl FnMut(u32, u32) -> [u8; 3]) -> RgbaImage {
        let mut data = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = f(x, y);
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
        RgbaImage::new(w, h, data).expect("valid")
    }

    #[test]
    fn soft_edge_gives_two_colors() {
        // Black to white over a 6 px ramp: the gray ramp must not become extra colors.
        let img = image(64, 32, |x, _| {
            let v = (x.saturating_sub(29) * 255 / 6).min(255) as u8;
            [v, v, v]
        });
        let q = quantize(&img, &Options::default());
        assert_eq!(q.palette.len(), 2, "palette: {:?}", q.palette);
    }

    #[test]
    fn noise_is_absorbed() {
        let mut s = 0x1234_5678u32;
        let img = image(64, 64, |x, _| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            let n = (s % 33) as i32 - 16;
            let base = if x < 32 { 40 } else { 210 };
            let v = (base + n).clamp(0, 255) as u8;
            [v, v / 2, 30]
        });
        let q = quantize(&img, &Options::default());
        assert_eq!(q.palette.len(), 2, "palette: {:?}", q.palette);
        let distinct: std::collections::BTreeSet<_> = q.labels[..32].iter().collect();
        assert_eq!(distinct.len(), 1);
    }

    #[test]
    fn thin_line_is_kept() {
        // A 1 px black line on white has no flat pixel of its own.
        let img = image(48, 48, |x, _| if x == 20 { [0, 0, 0] } else { [255, 255, 255] });
        let q = quantize(&img, &Options::default());
        assert_eq!(q.palette.len(), 2, "palette: {:?}", q.palette);
        let black = q.labels[20];
        assert!((0..48).all(|y| q.labels[y * 48 + 20] == black));
        assert_ne!(q.labels[0], black);
    }

    fn six_bands() -> RgbaImage {
        image(60, 20, |x, _| match x / 10 {
            0 => [255, 0, 0],
            1 => [0, 160, 0],
            2 => [0, 0, 255],
            3 => [250, 220, 0],
            4 => [0, 0, 0],
            _ => [255, 255, 255],
        })
    }

    #[test]
    fn max_colors_limits_the_palette() {
        let auto = quantize(&six_bands(), &Options::default());
        assert_eq!(auto.info.sources.len(), 6);
        assert_eq!(auto.info.groups.len(), 6);
        let three = quantize(&six_bands(), &Options { max_colors: 3, ..Options::default() });
        assert_eq!(three.info.sources.len(), 6, "sources do not depend on max_colors");
        assert_eq!(three.info.groups.len(), 3);
        assert_eq!(three.info.key, auto.info.key);
        let source_colors: Vec<&str> = auto.info.sources.iter().map(|s| s.color.as_str()).collect();
        assert!(
            three.info.groups.iter().all(|g| source_colors.contains(&g.color.as_str())),
            "{:?}",
            three.info
        );
    }

    #[test]
    fn palette_edit_regroups_and_recolors() {
        let auto = quantize(&six_bands(), &Options::default());
        // Put every source in group 7 except the first one, which gets a custom color.
        let mut groups = vec![7; auto.info.sources.len()];
        groups[0] = 3;
        let mut colors = vec![None; 8];
        colors[3] = Some("#123456".to_owned());
        let edit = PaletteEdit { key: auto.info.key, groups, colors };
        let opts = Options { palette_edit: Some(edit.clone()), ..Options::default() };
        let q = quantize(&six_bands(), &opts);
        assert_eq!(q.info.groups.len(), 2);
        assert_eq!(q.info.groups[0].color, "#123456");
        assert!(q.info.groups[0].custom && !q.info.groups[1].custom);
        assert_eq!(q.info.sources[0].group, 0);
        assert!(q.info.sources[1..].iter().all(|s| s.group == 1));
        // A stale key is ignored.
        let stale =
            Options { palette_edit: Some(PaletteEdit { key: edit.key ^ 1, ..edit }), ..Options::default() };
        assert_eq!(quantize(&six_bands(), &stale).info.groups.len(), 6);
    }

    #[test]
    fn transparency_is_preserved() {
        let mut data = vec![0u8; 16 * 16 * 4];
        for p in data.as_chunks_mut::<4>().0.iter_mut().skip(128) {
            *p = [200, 30, 30, 255];
        }
        let q = quantize(&RgbaImage::new(16, 16, data).expect("valid"), &Options::default());
        assert!(q.labels[..128].iter().all(|&l| l == TRANSPARENT));
        assert_eq!(q.to_image().data()[3], 0);
    }
}
