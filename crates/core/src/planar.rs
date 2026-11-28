//! M1 step 2: planar map and SVG writer.
//!
//! Regions are the 4-connected components of the label map. Their boundaries run along pixel
//! cracks; a boundary is cut into **runs** at junctions (grid vertices where 3 or more crack
//! edges meet). Each run is fitted **once** ([`crate::fit`]) and used by both regions it
//! separates (reversed for one of them), so neighbors share exactly the same edge: no gap, no
//! overlap.
//!
//! Output modes:
//! - **Cutout**: every region with its holes; regions tile the plane.
//! - **Stacked**: regions are drawn from the largest outline to the smallest, each one without
//!   its holes (they are covered by the regions drawn later), so anti-aliasing seams between
//!   neighbors are hidden. Holes that contain transparency are kept.

use std::collections::HashMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::RgbaImage;
use crate::edges::EdgeMap;
use crate::fit::{
    CornerStyle, Curve, Fillets, FitParams, P, Prepared, Seg, arc_tolerance, fit_prepared, prepare,
};
use crate::guide::{CANDIDATE_GEOMETRY, Guide, GuideKind, GuideStatus, Guides};
use crate::options::CORNER_EDIT_REACH;
use crate::palette::{Quantized, TRANSPARENT};
use crate::{CornerEdit, CornerMode, Options, Stacking};

/// Component id of transparent pixels and of the outside of the image.
const NONE: u32 = u32::MAX;
const STEP: [(i64, i64); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

struct Grid<'a> {
    w: i64,
    h: i64,
    comp: &'a [u32],
}

impl Grid<'_> {
    fn at(&self, x: i64, y: i64) -> u32 {
        if x < 0 || y < 0 || x >= self.w || y >= self.h { NONE } else { self.comp[(y * self.w + x) as usize] }
    }

    /// (left, right) components of the directed crack edge leaving vertex (x, y) in direction `d`
    /// (0 = +x, 1 = +y, 2 = −x, 3 = −y; y points down, so "left" of +x is the pixel above).
    fn sides(&self, x: i64, y: i64, d: u8) -> (u32, u32) {
        match d {
            0 => (self.at(x, y - 1), self.at(x, y)),
            1 => (self.at(x, y), self.at(x - 1, y)),
            2 => (self.at(x - 1, y), self.at(x - 1, y - 1)),
            _ => (self.at(x - 1, y - 1), self.at(x, y - 1)),
        }
    }

    fn degree(&self, x: i64, y: i64) -> usize {
        (0..4).filter(|&d| self.vertex_edge_exists(x, y, d)).count()
    }

    fn vertex_edge_exists(&self, x: i64, y: i64, d: u8) -> bool {
        let (nx, ny) = (x + STEP[d as usize].0, y + STEP[d as usize].1);
        if nx < 0 || ny < 0 || nx > self.w || ny > self.h {
            return false;
        }
        let (l, r) = self.sides(x, y, d);
        l != r
    }

    /// Pixel coordinates (left, right) of the directed crack edge, as in [`Grid::sides`].
    fn side_pixels(x: i64, y: i64, d: u8) -> ((i64, i64), (i64, i64)) {
        match d {
            0 => ((x, y - 1), (x, y)),
            1 => ((x, y), (x - 1, y)),
            2 => ((x - 1, y), (x - 1, y - 1)),
            _ => ((x - 1, y - 1), (x, y - 1)),
        }
    }

    /// Undirected edge id and whether `d` is its canonical direction (+x or +y).
    fn edge_id(&self, x: i64, y: i64, d: u8) -> (u64, bool) {
        let v = |x: i64, y: i64| (y * (self.w + 1) + x) as u64;
        match d {
            0 => (v(x, y) * 2, true),
            1 => (v(x, y) * 2 + 1, true),
            2 => (v(x - 1, y) * 2, false),
            _ => (v(x, y - 1) * 2 + 1, false),
        }
    }
}

/// 4-connected components of the label map; transparent pixels get [`NONE`].
fn components(q: &Quantized) -> (Vec<u32>, Vec<u16>) {
    let (w, h) = (q.width as usize, q.height as usize);
    let mut comp = vec![NONE; w * h];
    let mut labels = Vec::new();
    let mut stack = Vec::new();
    for start in 0..w * h {
        let label = q.labels[start];
        if comp[start] != NONE || label == TRANSPARENT {
            continue;
        }
        let id = labels.len() as u32;
        labels.push(label);
        comp[start] = id;
        stack.push(start);
        while let Some(i) = stack.pop() {
            let (x, y) = (i % w, i / w);
            let n = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in n.into_iter().flatten() {
                if comp[j] == NONE && q.labels[j] == label {
                    comp[j] = id;
                    stack.push(j);
                }
            }
        }
    }
    (comp, labels)
}

/// A boundary cycle of one region: runs (index into the run table, reversed?) in order.
struct Cycle {
    runs: Vec<(usize, bool)>,
    /// Signed area of the crack polygon (negative = outer boundary, positive = hole).
    area: f64,
    /// First pixel on the other side of the boundary (for the hole transparency test).
    outside_pixel: (i64, i64),
}

/// An isolated shape waiting to be finished: run index, grid vertices, boundary points.
type Isolated = (usize, Vec<(i64, i64)>, Vec<P>);

struct Runs {
    index: HashMap<u64, usize>,
    prepared: Vec<Prepared>,
    /// Geometry setting (circles, ellipses, symmetries of isolated shapes; 0 = off).
    geometry: f64,
    /// Guides of whole shapes (circles, ellipses, symmetries) and the ones turned off.
    guides: Guides,
    /// Isolated shapes (closed runs away from the image frame), finished once all are known.
    isolated: Vec<Isolated>,
    /// Whether each run touches the image frame.
    border: Vec<bool>,
}

/// Corner candidates of the whole image and how many are sharp.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CornerStats {
    pub total: u32,
    pub sharp: u32,
    /// Every corner, for the editor (in no particular order).
    pub list: Vec<CornerInfo>,
    /// Every guide the tracing found (see [`crate::guide`]).
    pub guides: Vec<Guide>,
}

/// One corner of the result, as shown in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct CornerInfo {
    /// Apex (image px): where the corner is when sharp, and the key of its [`CornerEdit`].
    pub x: f32,
    pub y: f32,
    pub sharp: bool,
    /// Radius drawn when rounded (px), and the largest one that fits.
    pub radius: f32,
    pub max_radius: f32,
    /// Corners of the same closed outline with the same angle (the 4 corners of a square,
    /// the 5 tips of a star) share a group, edited together by default.
    pub group: Option<u32>,
    /// A [`CornerEdit`] applies to this corner.
    pub edited: bool,
    /// Unit directions of travel along the incoming and outgoing sides (to draw the corner).
    pub din: [f32; 2],
    pub dout: [f32; 2],
}

/// In automatic mode, a corner is sharp when the boundary passes this close to its apex...
const AUTO_SHARP_GAP: f64 = 0.75;
/// ... or when the radius that would round it is this small (acute tips: anti-aliasing cuts
/// their point, so their gap is large even when they are sharp; small stars need 1 px).
const AUTO_SHARP_RADIUS: f64 = 1.0;
/// Corners of one outline whose turns differ by less than this (radians) form a group.
const GROUP_TURN: f64 = 0.2;

/// Decide how every corner is drawn, over the whole image (the sharp count is global), then
/// apply the per-corner edits.
fn decide_corners(runs: &[Prepared], options: &Options) -> (Vec<Vec<CornerStyle>>, CornerStats) {
    let groups = corner_groups(runs);
    let mut sharp: Vec<Vec<bool>> = runs.iter().map(|r| vec![false; r.corners.len()]).collect();
    let mut all: Vec<(usize, usize)> =
        runs.iter().enumerate().flat_map(|(r, run)| (0..run.corners.len()).map(move |c| (r, c))).collect();
    let corner = |&(r, c): &(usize, usize)| runs[r].corners[c];
    let min_turn = (25.0 + f64::from(options.smoothness) * 110.0).to_radians();
    match (options.corners.sharp_count, options.corners.mode) {
        (Some(n), _) => {
            // The n corners closest to their apex (then the sharpest turns) are sharp.
            all.sort_by(|a, b| {
                let (ca, cb) = (corner(a), corner(b));
                ca.gap.total_cmp(&cb.gap).then(cb.turn.total_cmp(&ca.turn))
            });
            for &(r, c) in all.iter().take(n as usize) {
                sharp[r][c] = true;
            }
        }
        (None, CornerMode::Sharp) => sharp.iter_mut().for_each(|s| s.fill(true)),
        (None, CornerMode::Round) => {}
        (None, CornerMode::Automatic) => {
            for &(r, c) in &all {
                let k = runs[r].corners[c];
                let close = k.gap <= AUTO_SHARP_GAP || runs[r].radii(c).0 <= AUTO_SHARP_RADIUS;
                sharp[r][c] = close && k.turn >= min_turn;
            }
            // Corners of one group (the tips of a star) get one decision: the majority's.
            for (r, groups) in groups.iter().enumerate() {
                let mut votes: HashMap<u32, (u32, u32)> = HashMap::new();
                for (c, g) in groups.iter().enumerate() {
                    if let Some(g) = g {
                        let v = votes.entry(*g).or_default();
                        v.0 += u32::from(sharp[r][c]);
                        v.1 += 1;
                    }
                }
                for (c, g) in groups.iter().enumerate() {
                    if let Some((yes, all)) = g.and_then(|g| votes.get(&g)) {
                        sharp[r][c] = 2 * yes >= *all;
                    }
                }
            }
        }
    }

    let rel = f64::from(options.corners.radius);
    let mut styles: Vec<Vec<CornerStyle>> = runs
        .iter()
        .zip(&sharp)
        .map(|(run, sharp)| {
            (0..run.corners.len())
                .map(|c| match (sharp[c], rel.abs() < 1e-6) {
                    (true, _) => CornerStyle::Sharp,
                    (false, true) => CornerStyle::Free,
                    (false, false) => CornerStyle::Arc(run.relative_radius(c, rel)),
                })
                .collect()
        })
        .collect();
    let mut edited: Vec<Vec<bool>> = runs.iter().map(|r| vec![false; r.corners.len()]).collect();
    for (r, c, edit) in match_edits(runs, &options.corners.edits) {
        edited[r][c] = true;
        styles[r][c] = match (edit.sharp, edit.radius) {
            (true, _) => CornerStyle::Sharp,
            (false, Some(radius)) => CornerStyle::Arc(f64::from(radius).min(runs[r].radii(c).1)),
            (false, None) if rel.abs() < 1e-6 => CornerStyle::Free,
            (false, None) => CornerStyle::Arc(runs[r].relative_radius(c, rel)),
        };
    }

    let mut list = Vec::with_capacity(all.len());
    for (r, run) in runs.iter().enumerate() {
        for (c, k) in run.corners.iter().enumerate() {
            let (r_fit, r_max) = run.radii(c);
            let (is_sharp, radius) = match styles[r][c] {
                CornerStyle::Sharp => (true, 0.0),
                CornerStyle::Free => (false, r_fit),
                CornerStyle::Arc(radius) => (false, radius),
            };
            list.push(CornerInfo {
                x: k.apex.x as f32,
                y: k.apex.y as f32,
                sharp: is_sharp,
                radius: radius as f32,
                max_radius: r_max as f32,
                group: groups[r][c],
                edited: edited[r][c],
                din: [k.din.x as f32, k.din.y as f32],
                dout: [k.dout.x as f32, k.dout.y as f32],
            });
        }
    }
    let count = list.iter().filter(|k| k.sharp).count() as u32;
    (styles, CornerStats { total: list.len() as u32, sharp: count, list, guides: Vec::new() })
}

/// (run, corner, edit) for every edit that finds a corner within reach; the nearest corner
/// wins, and a later edit of the same corner replaces an earlier one.
fn match_edits<'a>(runs: &[Prepared], edits: &'a [CornerEdit]) -> Vec<(usize, usize, &'a CornerEdit)> {
    if edits.is_empty() {
        return Vec::new();
    }
    let reach = f64::from(CORNER_EDIT_REACH);
    let cell = |x: f64, y: f64| ((x / reach).floor() as i64, (y / reach).floor() as i64);
    let mut index: HashMap<(i64, i64), Vec<(usize, usize)>> = HashMap::new();
    for (r, run) in runs.iter().enumerate() {
        for (c, k) in run.corners.iter().enumerate() {
            index.entry(cell(k.apex.x, k.apex.y)).or_default().push((r, c));
        }
    }
    let mut out = Vec::new();
    for edit in edits {
        let p = P::new(f64::from(edit.x), f64::from(edit.y));
        let (cx, cy) = cell(p.x, p.y);
        let mut best: Option<((usize, usize), f64)> = None;
        for key in (cx - 1..=cx + 1).flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y))) {
            for &(r, c) in index.get(&key).into_iter().flatten() {
                let d = (runs[r].corners[c].apex - p).len();
                if d <= reach && best.is_none_or(|(_, b)| d < b) {
                    best = Some(((r, c), d));
                }
            }
        }
        if let Some(((r, c), _)) = best {
            out.push((r, c, edit));
        }
    }
    out
}

/// Group the corners of each closed run that have nearly the same turn.
fn corner_groups(runs: &[Prepared]) -> Vec<Vec<Option<u32>>> {
    // Nearly the same turn, and alike: both sharp-ish or both roundings of a similar size.
    let alike = |a: &crate::fit::Corner, b: &crate::fit::Corner| {
        let (ga, gb) = (a.gap.max(0.3), b.gap.max(0.3));
        b.turn - a.turn < GROUP_TURN && a.is_fillet() == b.is_fillet() && ga.max(gb) <= 2.0 * ga.min(gb)
    };
    let mut next = 0u32;
    runs.iter()
        .map(|run| {
            let mut groups = vec![None; run.corners.len()];
            if !run.closed {
                return groups;
            }
            let mut order: Vec<usize> = (0..run.corners.len()).collect();
            order.sort_by(|&a, &b| run.corners[a].turn.total_cmp(&run.corners[b].turn));
            let mut i = 0;
            while i < order.len() {
                let mut j = i + 1;
                while j < order.len() && alike(&run.corners[order[j - 1]], &run.corners[order[j]]) {
                    j += 1;
                }
                if j - i >= 2 {
                    for &c in &order[i..j] {
                        groups[c] = Some(next);
                    }
                    next += 1;
                }
                i = j;
            }
            groups
        })
        .collect()
}

/// Trace all regions and write the SVG document.
pub fn to_svg(
    q: &Quantized,
    image: Option<&RgbaImage>,
    options: &Options,
    params: &FitParams,
) -> (String, CornerStats) {
    let (comp, comp_labels) = components(q);
    let grid = Grid { w: i64::from(q.width), h: i64::from(q.height), comp: &comp };
    let edge_point = EdgePoint { q, image, w: grid.w, h: grid.h, edges: image.map(|i| EdgeMap::new(i, q)) };
    let mut runs = Runs {
        index: HashMap::new(),
        prepared: Vec::new(),
        geometry: f64::from(options.geometry),
        guides: Guides::new(&options.disabled_guides, &options.forced_guides),
        isolated: Vec::new(),
        border: Vec::new(),
    };
    let mut cycles: Vec<Vec<Cycle>> = (0..comp_labels.len()).map(|_| Vec::new()).collect();
    let mut visited = vec![false; ((grid.w + 1) * (grid.h + 1) * 4) as usize];

    for y in 0..=grid.h {
        for x in 0..=grid.w {
            for d in 0..4u8 {
                if !grid.vertex_edge_exists(x, y, d) {
                    continue;
                }
                let (id, canonical) = grid.edge_id(x, y, d);
                if visited[(id * 2 + u64::from(!canonical)) as usize] {
                    continue;
                }
                let (left, _) = grid.sides(x, y, d);
                if left == NONE {
                    continue;
                }
                let cycle = trace_cycle(&grid, &edge_point, (x, y, d), left, &mut visited, &mut runs);
                cycles[left as usize].push(cycle);
            }
        }
    }

    finish_isolated(&mut runs);
    regularize_regions(&cycles, &mut runs, (grid.w as f64, grid.h as f64));
    // Circles and ellipses on whole region outlines made of several runs.
    let region_shapes = region_shapes(&cycles, &mut runs);
    let (styles, mut corner_stats) = decide_corners(&runs.prepared, options);
    let (curves, run_guides) = fit_runs(&runs.prepared, &styles, &region_shapes, options, *params);
    corner_stats.guides = std::mem::take(&mut runs.guides.found);
    corner_stats.guides.extend(run_guides);
    // The image frame is not part of the drawing.
    let (w, h) = (
        f32::from(u16::try_from(q.width).unwrap_or(u16::MAX)),
        f32::from(u16::try_from(q.height).unwrap_or(u16::MAX)),
    );
    corner_stats.guides.retain(|g| {
        g.kind != GuideKind::Line || (g.x > 0.01 && g.y > 0.01 && g.x < w - 0.01 && g.y < h - 0.01)
    });

    let svg = write_svg(q, &grid, &cycles, &comp_labels, &curves, options);
    (svg, corner_stats)
}

/// Write the SVG document: one path per region, largest outline first.
fn write_svg(
    q: &Quantized,
    grid: &Grid,
    cycles: &[Vec<Cycle>],
    comp_labels: &[u16],
    curves: &[Curve],
    options: &Options,
) -> String {
    let stacking = options.stacking;
    // Draw order: largest outline first.
    let outer_area = |c: &[Cycle]| c.iter().map(|k| -k.area).fold(0.0, f64::max);
    let mut order: Vec<usize> = (0..cycles.len()).filter(|&c| !cycles[c].is_empty()).collect();
    order.sort_by(|&a, &b| outer_area(&cycles[b]).total_cmp(&outer_area(&cycles[a])));

    // Gap filler: where a region meets one drawn after it, it gets a thin stroke of its own
    // colour along that boundary. The later region hides it, except on the seam itself, where
    // anti-aliasing would otherwise let the background show through (light or dark lines
    // between two shapes).
    let mut position = vec![usize::MAX; cycles.len()];
    for (i, &c) in order.iter().enumerate() {
        position[c] = i;
    }
    let mut sides: Vec<Vec<usize>> = vec![Vec::new(); curves.len()];
    for (c, list) in cycles.iter().enumerate() {
        for &(idx, _) in list.iter().flat_map(|cycle| &cycle.runs) {
            sides[idx].push(c);
        }
    }

    let has_transparency = q.labels.contains(&TRANSPARENT);
    let mut stamp = vec![u32::MAX; grid.comp.len()];
    let mut svg = String::new();
    let _ = write!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">"#,
        w = q.width,
        h = q.height
    );
    svg.push('\n');
    for c in order {
        let [r, g, b] = q.palette[comp_labels[c] as usize];
        let mut d = String::new();
        let mut seams = String::new();
        for cycle in &cycles[c] {
            let is_hole = cycle.area > 0.0;
            let keep = match stacking {
                Stacking::Cutout => true,
                Stacking::Stacked => {
                    !is_hole
                        || (has_transparency
                            && hole_has_transparency(grid, c as u32, cycle.outside_pixel, &mut stamp))
                }
            };
            if keep {
                write_cycle(&mut d, cycle, curves);
                if options.fill_gaps {
                    for &(idx, _) in &cycle.runs {
                        if sides[idx].iter().any(|&o| o != c && position[o] > position[c]) {
                            write_run(&mut seams, &curves[idx]);
                        }
                    }
                }
            }
        }
        let _ = writeln!(svg, r##"<path fill="#{r:02x}{g:02x}{b:02x}" d="{d}"/>"##);
        if !seams.is_empty() {
            let _ = writeln!(
                svg,
                r##"<path fill="none" stroke="#{r:02x}{g:02x}{b:02x}" stroke-width="{GAP_FILL_WIDTH}" stroke-linejoin="round" d="{seams}"/>"##
            );
        }
    }
    svg.push_str("</svg>\n");
    svg
}

/// Fit every run (or take the shape that replaces it), then refit those with fillets so that
/// close radii become equal. Returns the curves and the guides of the runs (from their last fit).
fn fit_runs(
    prepared: &[Prepared],
    styles: &[Vec<CornerStyle>],
    region_shapes: &[Option<Curve>],
    options: &Options,
    params: FitParams,
) -> (Vec<Curve>, Vec<Guide>) {
    let mut fillets = Fillets::default();
    let mut with_fillets = Vec::new();
    let mut curves: Vec<Curve> = Vec::with_capacity(prepared.len());
    let mut run_guides: Vec<Vec<Guide>> = Vec::with_capacity(prepared.len());
    let mut guides = Guides::new(&options.disabled_guides, &options.forced_guides);
    for (i, (run, styles)) in prepared.iter().zip(styles).enumerate() {
        if let Some(curve) = &region_shapes[i] {
            curves.push(curve.clone());
            run_guides.push(Vec::new());
            continue;
        }
        let before = fillets.found.len();
        curves.push(fit_prepared(run, styles, params, &mut fillets, &mut guides));
        run_guides.push(std::mem::take(&mut guides.found));
        if fillets.found.len() > before {
            with_fillets.push(i);
        }
    }
    fillets.snap = Fillets::clusters(&fillets.found);
    if !fillets.snap.is_empty() {
        for i in with_fillets {
            curves[i] = fit_prepared(&prepared[i], &styles[i], params, &mut fillets, &mut guides);
            run_guides[i] = std::mem::take(&mut guides.found);
        }
    }
    // Check every geometric fit against the measured boundary: one that strays from it (a
    // shape imposed where the image does not have it) is replaced by the plain tracing, and
    // its guides are dropped.
    let limit = params.tolerance + VERIFY_SLACK;
    let plain = FitParams { plain: true, ..params };
    for (i, run) in prepared.iter().enumerate() {
        // Only runs that got some geometry: a plain fit follows the points by construction.
        let geometric = region_shapes[i].is_some()
            || run.has_shape()
            || run.corners.iter().any(crate::fit::Corner::is_fillet)
            || run_guides[i].iter().any(|g| matches!(g.status, GuideStatus::Followed | GuideStatus::Forced));
        if run.soft || !geometric || run.deviation(&styles[i], &curves[i]) <= limit {
            continue;
        }
        let mut scratch = Guides::default();
        curves[i] = fit_prepared(run, &styles[i], plain, &mut Fillets::default(), &mut scratch);
        run_guides[i].clear();
    }
    (curves, run_guides.into_iter().flatten().collect())
}

/// Corners on the image frame are not shape corners: always sharp, never counted. A rounding
/// of the drawing whose apex falls on the frame stays a corner of the drawing, unless the
/// boundary really passes by the apex (a frame corner seen as a tiny rounding).
fn split_frame_corners(prepared: &mut Prepared, (w, h): (f64, f64)) {
    let on_frame = |p: P| p.x <= 0.01 || p.y <= 0.01 || p.x >= w - 0.01 || p.y >= h - 0.01;
    let passes_by = |apex: P| prepared.pts().iter().any(|&p| (p - apex).len() <= 1.0);
    let (frame, corners): (Vec<_>, Vec<_>) =
        prepared.corners.iter().partition(|c| on_frame(c.apex) && (!c.is_fillet() || passes_by(c.apex)));
    (prepared.frame, prepared.corners) = (frame, corners);
}

/// A geometric fit may stray this far (px) beyond the fit tolerance from the measured
/// boundary (its points carry some noise); farther, it is not what the image shows.
const VERIFY_SLACK: f64 = 0.6;

/// Below this sharpness (see [`EdgeMap::sharpness`]), a boundary is soft: a band boundary
/// inside a gradient rather than an edge of the drawing.
const SOFT_EDGE: f64 = 0.45;

/// Region outlines with fewer points (hatching strokes, specks) are not regularized.
const MIN_REGULARIZED_POINTS: usize = 32;

/// The symmetry of a closed outline, if any, and whether it is within the geometry setting's
/// tolerance (otherwise it is a candidate). The loose search goes first: it fails for most
/// outlines, and then nothing else is needed.
fn find_symmetry(pts: &[P], geometry: f64) -> (Option<crate::shape::Symmetrized>, bool) {
    let Some(loose) = crate::shape::symmetrize(pts, CANDIDATE_GEOMETRY) else { return (None, false) };
    match crate::shape::symmetrize(pts, geometry) {
        Some(strict) => (Some(strict), true),
        None => (Some(loose), false),
    }
}

/// Points blended from the original toward the regularized outline next to a junction (the
/// junction itself does not move: other runs end there).
const JUNCTION_BLEND: f64 = 4.0;

/// Symmetries and repeats of region outlines made of several runs (shapes that touch other
/// colours). The outline is regularized as a whole (repeats first, then symmetry); each run
/// takes its part of the result, its end junctions staying in place, and is prepared again. A
/// run shared by two regularized outlines follows the first one.
fn regularize_regions(cycles: &[Vec<Cycle>], runs: &mut Runs, size: (f64, f64)) {
    // Outlines of two runs or more, none along the image frame.
    let outlines: Vec<&Cycle> = cycles
        .iter()
        .flatten()
        .filter(|c| {
            c.runs.len() >= 2 && c.runs.iter().all(|&(i, _)| !runs.border[i] && !runs.prepared[i].soft)
        })
        .filter(|c| {
            c.runs.iter().map(|&(i, _)| runs.prepared[i].pts().len()).sum::<usize>() >= MIN_REGULARIZED_POINTS
        })
        .collect();
    let loops: Vec<Vec<P>> = outlines
        .iter()
        .map(|cycle| {
            let mut pts = Vec::new();
            for &(idx, reversed) in &cycle.runs {
                let mut p = runs.prepared[idx].pts().to_vec();
                if reversed {
                    p.reverse();
                }
                p.pop(); // the next run starts there (the first one, for the last run)
                pts.extend(p);
            }
            pts
        })
        .collect();
    let repeated = crate::repeat::unify(&loops, runs.geometry, &mut runs.guides);
    let mut claimed = vec![false; runs.prepared.len()];
    for ((cycle, outline), new) in outlines.iter().zip(&loops).zip(repeated) {
        let outline = new.unwrap_or_else(|| outline.clone());
        let (found, within) = find_symmetry(&outline, runs.geometry);
        let target = match found {
            Some(sym) if runs.guides.add(GuideKind::Symmetry, sym.top, sym.guide.clone(), within) => {
                crate::shape::aligned(&outline, &sym)
            }
            _ => outline,
        };
        // Hand each run its part (unchanged runs get back their own points).
        let total = target.len();
        let mut base = 0;
        for &(idx, reversed) in &cycle.runs {
            let mut orig = runs.prepared[idx].pts().to_vec();
            if reversed {
                orig.reverse();
            }
            let m = orig.len() - 1;
            if !claimed[idx] && total > 0 {
                let mut pts: Vec<P> = (0..=m)
                    .map(|j| {
                        let w = (j.min(m - j) as f64 / JUNCTION_BLEND).min(1.0);
                        orig[j] + (target[(base + j) % total] - orig[j]) * w
                    })
                    .collect();
                if pts.iter().zip(&orig).any(|(a, b)| (*a - *b).len() > 1e-6) {
                    if reversed {
                        pts.reverse();
                    }
                    let mut prepared = prepare(
                        &vec![(0, 0); pts.len() - 1],
                        &pts[1..pts.len() - 1],
                        (pts[0], pts[pts.len() - 1]),
                        false,
                        None,
                    );
                    prepared.add_fillet_corners(arc_tolerance(runs.geometry), &runs.guides);
                    split_frame_corners(&mut prepared, size);
                    runs.prepared[idx] = prepared;
                }
                claimed[idx] = true;
            }
            base += m;
        }
    }
}

/// Finish the isolated shapes: copies of one shape get the same outline, then each shape that
/// is nearly symmetric is made exactly symmetric, and circles and ellipses become exact.
fn finish_isolated(runs: &mut Runs) {
    let isolated = std::mem::take(&mut runs.isolated);
    let loops: Vec<Vec<P>> = isolated.iter().map(|(_, _, mids)| mids.clone()).collect();
    let repeated = crate::repeat::unify(&loops, runs.geometry, &mut runs.guides);
    for ((idx, vertices, mids), new) in isolated.into_iter().zip(repeated) {
        let mids = new.unwrap_or(mids);
        let (found, within) = find_symmetry(&mids, runs.geometry);
        let symmetry_guide = found.as_ref().map(|_| runs.guides.found.len());
        let (mids, symmetry) = match found {
            Some(sym) if runs.guides.add(GuideKind::Symmetry, sym.top, sym.guide.clone(), within) => {
                (sym.pts, Some(sym.kind))
            }
            _ => (mids, None),
        };
        let mut prepared = prepare(&vertices, &mids, (P::default(), P::default()), true, symmetry);
        prepared.detect_shape(runs.geometry, &mut runs.guides);
        prepared.add_fillet_corners(arc_tolerance(runs.geometry), &runs.guides);
        // A circle or an ellipse shows as such: its symmetry axes would only clutter the view.
        if prepared.has_shape()
            && let Some(i) = symmetry_guide
        {
            runs.guides.found.remove(i);
        }
        runs.prepared[idx] = prepared;
    }
}

/// Circles and ellipses that whole region outlines (cycles of two runs or more) follow. Each run
/// of such an outline becomes the matching part of the shape, between its own end junctions
/// (shared with the other runs that meet there, so nothing opens). A run claimed by two shapes
/// keeps the best-fitting one. Returns the curve of each run that a shape replaces; those runs
/// lose their corner candidates.
fn region_shapes(cycles: &[Vec<Cycle>], runs: &mut Runs) -> Vec<Option<Curve>> {
    let mut fits = Vec::new();
    for cycle in cycles
        .iter()
        .flatten()
        .filter(|c| c.runs.len() >= 2 && c.runs.iter().all(|&(i, _)| !runs.prepared[i].soft))
    {
        let mut pts = Vec::new();
        for &(idx, reversed) in &cycle.runs {
            let mut p = runs.prepared[idx].pts().to_vec();
            if reversed {
                p.reverse();
            }
            // Each run starts where the previous one ended.
            let skip = usize::from(!pts.is_empty());
            pts.extend(p.into_iter().skip(skip));
        }
        let strict = crate::shape::fit_round(&pts, runs.geometry);
        let within = strict.is_some();
        if let Some((ellipse, _)) = strict.or_else(|| crate::shape::fit_round(&pts, CANDIDATE_GEOMETRY)) {
            let rms =
                (pts.iter().map(|&p| ellipse.distance(p).powi(2)).sum::<f64>() / pts.len() as f64).sqrt();
            fits.push((rms, ellipse, within, cycle.runs.iter().map(|&(i, _)| i).collect::<Vec<_>>()));
        }
    }
    fits.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<Option<Curve>> = vec![None; runs.prepared.len()];
    // Which shape each run follows (the best fitting one that claims it).
    let mut claims: Vec<(crate::shape::Ellipse, Vec<usize>)> = Vec::new();
    for (_, ellipse, within, members) in fits {
        // Both sides of an outline find the same shape: one guide.
        let top = ellipse.top();
        let seen =
            runs.guides.found.iter().any(|g| {
                g.kind == ellipse.kind() && (P::new(f64::from(g.x), f64::from(g.y)) - top).len() < 1.0
            });
        if seen || !runs.guides.add(ellipse.kind(), top, ellipse.guide(), within) {
            continue;
        }
        let mine: Vec<usize> = members.into_iter().filter(|&i| out[i].is_none()).collect();
        for &idx in &mine {
            out[idx] = Some(Curve { start: P::default(), segs: Vec::new() }); // claimed
        }
        claims.push((ellipse, mine));
    }
    // The junctions of a shape's runs move onto the shape (the first shape that reaches a
    // junction places it), and every run that ends there follows: the outline is exact all
    // the way round, and neighbours stay watertight.
    let key = |p: P| (p.x.to_bits(), p.y.to_bits());
    let mut moves: HashMap<(u64, u64), P> = HashMap::new();
    for (ellipse, mine) in &claims {
        for &idx in mine {
            let p = runs.prepared[idx].pts();
            for end in [p[0], p[p.len() - 1]] {
                moves.entry(key(end)).or_insert_with(|| ellipse.nearest(end));
            }
        }
    }
    if !moves.is_empty() {
        for run in runs.prepared.iter_mut().filter(|r| !r.closed) {
            let p = run.pts();
            let (a, b) = (p[0], p[p.len() - 1]);
            let (na, nb) = (moves.get(&key(a)).copied(), moves.get(&key(b)).copied());
            if na.is_some() || nb.is_some() {
                run.set_ends(na.unwrap_or(a), nb.unwrap_or(b));
            }
        }
    }
    for (ellipse, mine) in &claims {
        for &idx in mine {
            let run = &mut runs.prepared[idx];
            let p = run.pts();
            out[idx] = Some(ellipse.arc(p[0], p[p.len() - 1], p[p.len() / 2]));
            run.corners.clear();
        }
    }
    out
}

#[allow(clippy::too_many_lines)]
fn trace_cycle(
    grid: &Grid,
    edge_point: &EdgePoint,
    start: (i64, i64, u8),
    c: u32,
    visited: &mut [bool],
    runs: &mut Runs,
) -> Cycle {
    // 1. Walk the crack edges with `c` on the left (4-connectivity turn rule).
    let mut edges = Vec::new();
    let (mut x, mut y, mut d) = start;
    loop {
        let (id, canonical) = grid.edge_id(x, y, d);
        visited[(id * 2 + u64::from(!canonical)) as usize] = true;
        edges.push((x, y, d));
        x += STEP[d as usize].0;
        y += STEP[d as usize].1;
        let (fl, fr) = grid.sides(x, y, d);
        d = if fl != c {
            (d + 3) % 4
        } else if fr == c {
            (d + 1) % 4
        } else {
            d
        };
        if (x, y, d) == start {
            break;
        }
    }
    let n = edges.len();
    let mut area = 0.0;
    for (i, &(x0, y0, _)) in edges.iter().enumerate() {
        let (x1, y1, _) = edges[(i + 1) % n];
        area += (x0 * y1 - x1 * y0) as f64;
    }
    let (sx, sy, sd) = edges[0];
    let outside = match sd {
        0 => (sx, sy),
        1 => (sx - 1, sy),
        2 => (sx - 1, sy - 1),
        _ => (sx, sy - 1),
    };

    // 2. Cut at junctions; fit each run once, keyed by its smallest edge id.
    let junctions: Vec<usize> = (0..n).filter(|&i| grid.degree(edges[i].0, edges[i].1) >= 3).collect();
    let mut out = Vec::new();
    if junctions.is_empty() {
        out.push(lookup_run(grid, edge_point, &edges, true, runs));
    } else {
        for (k, &a) in junctions.iter().enumerate() {
            let b = junctions[(k + 1) % junctions.len()];
            let len = (b + n - a - 1) % n + 1;
            let run: Vec<_> = (0..len).map(|i| edges[(a + i) % n]).collect();
            out.push(lookup_run(grid, edge_point, &run, false, runs));
        }
    }
    Cycle { runs: out, area: area / 2.0, outside_pixel: outside }
}

/// Find or fit the run made of `edges` (as traversed); returns (run index, reversed?).
fn lookup_run(
    grid: &Grid,
    edge_point: &EdgePoint,
    edges: &[(i64, i64, u8)],
    closed: bool,
    runs: &mut Runs,
) -> (usize, bool) {
    let ids: Vec<(u64, bool)> = edges.iter().map(|&(x, y, d)| grid.edge_id(x, y, d)).collect();
    let (pos, &(key, canonical)) =
        ids.iter().enumerate().min_by_key(|(_, (id, _))| *id).expect("a run has at least one edge");
    let reversed = !canonical;
    if let Some(&idx) = runs.index.get(&key) {
        return (idx, reversed);
    }
    // Vertices in canonical orientation.
    let end = |&(x, y, d): &(i64, i64, u8)| (x + STEP[d as usize].0, y + STEP[d as usize].1);
    let mut vertices: Vec<(i64, i64)> = edges.iter().map(|&(x, y, _)| (x, y)).collect();
    if !closed {
        vertices.push(end(&edges[edges.len() - 1]));
    }
    if closed {
        vertices.rotate_left(pos);
    }
    if reversed {
        vertices.reverse();
        if closed {
            // [v0, v1, …] traversed backwards: the canonical loop starts at v1, then v0.
            vertices.rotate_right(2);
        }
    }
    let n = vertices.len();
    let count = if closed { n } else { n - 1 };
    let mids: Vec<P> = (0..count).map(|i| edge_point.at(vertices[i], vertices[(i + 1) % n])).collect();
    let ends = (edge_point.junction(grid, vertices[0]), edge_point.junction(grid, vertices[n - 1]));
    let mut sharp: Vec<f64> =
        (0..count).filter_map(|i| edge_point.sharpness(vertices[i], vertices[(i + 1) % n])).collect();
    sharp.sort_by(f64::total_cmp);
    let sharpness = sharp.get(sharp.len() / 2).copied().unwrap_or(1.0);
    let idx = runs.prepared.len();
    // Boundaries along the image frame keep their straight frame edges as they are; isolated
    // shapes are finished once all of them are known (see `finish_isolated`).
    let on_border = vertices.iter().any(|&(x, y)| x == 0 || y == 0 || x == grid.w || y == grid.h);
    let soft = sharpness < SOFT_EDGE;
    if closed && !on_border && !soft {
        runs.isolated.push((idx, vertices.clone(), mids.clone()));
    }
    let mut prepared = prepare(&vertices, &mids, ends, closed, None);
    prepared.soft = soft;
    prepared.add_fillet_corners(arc_tolerance(runs.geometry), &runs.guides);
    split_frame_corners(&mut prepared, (grid.w as f64, grid.h as f64));
    runs.prepared.push(prepared);
    runs.border.push(on_border);
    runs.index.insert(key, idx);
    (idx, reversed)
}

/// Where a boundary crosses a crack edge. Pixels on both sides are mixes of their two region
/// colors (anti-aliasing, blur); their coverage locates the 50 % transition between the two
/// pixel centers, so the boundary moves off the pixel grid by up to [`MAX_SHIFT`] px.
struct EdgePoint<'a> {
    q: &'a Quantized,
    image: Option<&'a RgbaImage>,
    w: i64,
    h: i64,
    /// Image gradient, to tell real edges from band boundaries inside gradients.
    edges: Option<EdgeMap>,
}

/// Largest sub-pixel shift: keeps neighboring boundaries from crossing.
const MAX_SHIFT: f64 = 0.45;
/// Largest distance of a pixel color from the segment between its two region colors, relative
/// to the segment length, for its coverage to be trusted.
const OFF_SEGMENT: f64 = 0.25;

impl EdgePoint<'_> {
    /// A junction sits at the mean of the boundary points of its incident crack edges, so the
    /// runs that meet there all end at the same sub-pixel position. Image-border vertices stay
    /// on the border.
    fn junction(&self, grid: &Grid, v: (i64, i64)) -> P {
        let vertex = P::new(v.0 as f64, v.1 as f64);
        if v.0 == 0 || v.1 == 0 || v.0 == self.w || v.1 == self.h || self.image.is_none() {
            return vertex;
        }
        let (mut sum, mut n) = (P::default(), 0.0);
        for d in 0..4u8 {
            if grid.vertex_edge_exists(v.0, v.1, d) {
                let s = STEP[d as usize];
                let m = self.at(v, (v.0 + s.0, v.1 + s.1));
                // The edge point lies half a pixel along the edge: keep only its normal shift.
                sum = sum + vertex + (m - (vertex + P::new(s.0 as f64 * 0.5, s.1 as f64 * 0.5)));
                n += 1.0;
            }
        }
        if n == 0.0 { vertex } else { sum * (1.0 / n) }
    }

    /// Sharpness of the crack edge from vertex `a` to vertex `b` (see [`EdgeMap::sharpness`]).
    fn sharpness(&self, a: (i64, i64), b: (i64, i64)) -> Option<f64> {
        let edges = self.edges.as_ref()?;
        let d = match (b.0 - a.0, b.1 - a.1) {
            (1, 0) => 0,
            (0, 1) => 1,
            (-1, 0) => 2,
            _ => 3,
        };
        let (l, r) = Grid::side_pixels(a.0, a.1, d);
        let label = |p: (i64, i64)| {
            if p.0 < 0 || p.1 < 0 || p.0 >= self.w || p.1 >= self.h {
                TRANSPARENT
            } else {
                self.q.labels[(p.1 * self.w + p.0) as usize]
            }
        };
        edges.sharpness(l, r, label(l), label(r))
    }

    fn at(&self, a: (i64, i64), b: (i64, i64)) -> P {
        let mid = P::new((a.0 + b.0) as f64 * 0.5, (a.1 + b.1) as f64 * 0.5);
        let d = match (b.0 - a.0, b.1 - a.1) {
            (1, 0) => 0,
            (0, 1) => 1,
            (-1, 0) => 2,
            _ => 3,
        };
        let (l, r) = Grid::side_pixels(a.0, a.1, d);
        match self.shift(l, r) {
            Some(e) => mid + P::new((r.0 - l.0) as f64, (r.1 - l.1) as f64) * e,
            None => mid,
        }
    }

    /// Offset of the transition from the crack toward `r` (in px), if both pixels are opaque
    /// and their region colors differ enough to measure coverage.
    fn shift(&self, l: (i64, i64), r: (i64, i64)) -> Option<f64> {
        let image = self.image?;
        let inside = |p: (i64, i64)| p.0 >= 0 && p.1 >= 0 && p.0 < self.w && p.1 < self.h;
        if !inside(l) || !inside(r) {
            return None;
        }
        let (il, ir) = ((l.1 * self.w + l.0) as usize, (r.1 * self.w + r.0) as usize);
        let (la, lb) = (self.q.labels[il], self.q.labels[ir]);
        if la == TRANSPARENT || lb == TRANSPARENT || la == lb {
            return None;
        }
        let color = |i: usize| {
            let px = &image.data()[i * 4..i * 4 + 3];
            [f64::from(px[0]), f64::from(px[1]), f64::from(px[2])]
        };
        let (cl, cr) = (color(il), color(ir));
        // Each region may group several source colors: measure against the pair that explains
        // both pixels best (a merged tint is not a mix of the group average).
        let mut best: Option<(f64, f64)> = None; // (worst relative residual, e)
        for a in &self.q.model[la as usize] {
            for b in &self.q.model[lb as usize] {
                let (ca, cb) = (a.map(f64::from), b.map(f64::from));
                let ab: [f64; 3] = std::array::from_fn(|k| cb[k] - ca[k]);
                let len2: f64 = ab.iter().map(|v| v * v).sum();
                if len2 < 40.0 * 40.0 {
                    continue;
                }
                // Fraction of color b in a pixel (projection on the a→b segment), and the
                // squared distance of the pixel from that segment relative to its length.
                let fit = |c: [f64; 3]| {
                    let t = (0..3).map(|k| ab[k] * (c[k] - ca[k])).sum::<f64>() / len2;
                    let t = t.clamp(0.0, 1.0);
                    let d2: f64 = (0..3).map(|k| (c[k] - ca[k] - ab[k] * t).powi(2)).sum();
                    (t, d2 / len2)
                };
                let ((tl, dl), (tr, dr)) = (fit(cl), fit(cr));
                let worst = dl.max(dr);
                if best.is_none_or(|(w, _)| worst < w) {
                    // b occupies the axis beyond e (crack at 0, l covers [-1, 0], r covers [0, 1]).
                    best = Some((worst, (1.0 - tr) - tl));
                }
            }
        }
        // A pixel far from every segment is not a mix of the two regions (a third color).
        let (worst, e) = best?;
        if worst > OFF_SEGMENT * OFF_SEGMENT {
            return None;
        }
        Some(e.clamp(-MAX_SHIFT, MAX_SHIFT))
    }
}

fn fmt(v: f64, out: &mut String) {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r }; // no "-0"
    let _ = write!(out, "{r}");
}

fn write_point(out: &mut String, p: P) {
    fmt(p.x, out);
    out.push(' ');
    fmt(p.y, out);
}

/// Width (px) of the gap filler strokes.
const GAP_FILL_WIDTH: f64 = 1.0;

/// One run as an open subpath.
fn write_run(d: &mut String, curve: &Curve) {
    d.push('M');
    write_point(d, curve.start);
    for seg in &curve.segs {
        match *seg {
            Seg::Line(e) => {
                d.push('L');
                write_point(d, e);
            }
            Seg::Cubic(c1, c2, e) => {
                d.push('C');
                write_point(d, c1);
                d.push(' ');
                write_point(d, c2);
                d.push(' ');
                write_point(d, e);
            }
        }
    }
}

fn write_cycle(d: &mut String, cycle: &Cycle, curves: &[Curve]) {
    let mut first = true;
    for &(idx, reversed) in &cycle.runs {
        let rev;
        let curve = if reversed {
            rev = curves[idx].reversed();
            &rev
        } else {
            &curves[idx]
        };
        if first {
            d.push('M');
            write_point(d, curve.start);
            first = false;
        }
        for seg in &curve.segs {
            match *seg {
                Seg::Line(e) => {
                    d.push('L');
                    write_point(d, e);
                }
                Seg::Cubic(c1, c2, e) => {
                    d.push('C');
                    write_point(d, c1);
                    d.push(' ');
                    write_point(d, c2);
                    d.push(' ');
                    write_point(d, e);
                }
            }
        }
    }
    d.push('Z');
}

/// Flood the hole of region `c` from `seed` (4-connected, not crossing `c`) looking for transparency.
fn hole_has_transparency(grid: &Grid, c: u32, seed: (i64, i64), stamp: &mut [u32]) -> bool {
    let mut stack = vec![seed];
    while let Some((x, y)) = stack.pop() {
        let v = grid.at(x, y);
        if v == NONE {
            return true;
        }
        let i = (y * grid.w + x) as usize;
        if v == c || stamp[i] == c {
            continue;
        }
        stamp[i] = c;
        stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quantized(w: u32, h: u32, f: impl Fn(u32, u32) -> u16) -> Quantized {
        let labels = (0..h).flat_map(|y| (0..w).map(move |x| (x, y))).map(|(x, y)| f(x, y)).collect();
        Quantized {
            width: w,
            height: h,
            palette: vec![[255, 255, 255], [200, 0, 0], [0, 0, 200]],
            labels,
            model: vec![vec![[255, 255, 255]], vec![[200, 0, 0]], vec![[0, 0, 200]]],
            info: crate::Palette::default(),
        }
    }

    fn params() -> FitParams {
        FitParams { tolerance: 0.7, arc_tol: 0.5, plain: false }
    }

    fn svg(q: &Quantized, stacking: Stacking) -> String {
        to_svg(q, None, &Options { stacking, fill_gaps: false, ..Options::default() }, &params()).0
    }

    #[test]
    fn square_in_background_cutout_and_stacked() {
        let q = quantized(20, 20, |x, y| u16::from((5..15).contains(&x) && (5..15).contains(&y)));
        let cut = svg(&q, Stacking::Cutout);
        assert_eq!(cut.matches("<path").count(), 2);
        // Background has an outer boundary and a hole in cutout mode.
        assert_eq!(cut.matches('M').count(), 3, "{cut}");
        let stacked = svg(&q, Stacking::Stacked);
        assert_eq!(stacked.matches('M').count(), 2, "{stacked}");
        assert!(stacked.find("#ffffff") < stacked.find("#c80000"), "{stacked}");
        assert!(stacked.contains("M5 5") || stacked.contains("L5 5"), "{stacked}");
    }

    #[test]
    fn neighbors_share_the_same_boundary() {
        // Three vertical bands: the two red/blue boundary points must appear in both paths.
        let q = quantized(30, 10, |x, _| (x / 10) as u16);
        let svg = svg(&q, Stacking::Cutout);
        let paths: Vec<&str> = svg.lines().filter(|l| l.starts_with("<path")).collect();
        let [white, red, blue] = paths[..] else { panic!("{svg}") };
        for p in ["10 0", "10 10"] {
            assert!(white.contains(p) && red.contains(p), "{p} in {svg}");
        }
        for p in ["20 0", "20 10"] {
            assert!(red.contains(p) && blue.contains(p), "{p} in {svg}");
        }
    }

    #[test]
    fn corner_edits_override_the_global_mode() {
        let q = quantized(20, 20, |x, y| u16::from((5..15).contains(&x) && (5..15).contains(&y)));
        let mut options = Options::default();
        options.corners.mode = CornerMode::Sharp;
        let (_, stats) = to_svg(&q, None, &options, &params());
        assert_eq!((stats.total, stats.sharp), (4, 4), "{stats:?}");
        // The 4 corners of the square are one group.
        let group = stats.list[0].group;
        assert!(group.is_some() && stats.list.iter().all(|k| k.group == group), "{stats:?}");

        // Round the top-left corner (edit placed slightly off its apex) with a 3 px radius.
        options.corners.edits = vec![CornerEdit { x: 5.3, y: 4.8, sharp: false, radius: Some(3.0) }];
        let (svg, stats) = to_svg(&q, None, &options, &params());
        assert_eq!(stats.sharp, 3, "{stats:?}");
        let k = stats.list.iter().find(|k| k.edited).expect("edited corner");
        assert!(!k.sharp && (k.radius - 3.0).abs() < 1e-6 && (k.x - 5.0).abs() < 0.1, "{k:?}");
        assert!(!svg.contains("M5 5") && !svg.contains("L5 5"), "{svg}");

        // An edit far from every corner is ignored.
        options.corners.edits = vec![CornerEdit { x: 10.0, y: 10.0, sharp: false, radius: None }];
        let (_, stats) = to_svg(&q, None, &options, &params());
        assert_eq!(stats.sharp, 4);
    }

    #[test]
    fn disc_across_two_backgrounds_becomes_a_circle() {
        // Red disc (r = 20) over a white/blue boundary: its outline has two runs.
        let q = quantized(80, 60, |x, y| {
            let (dx, dy) = (f64::from(x) + 0.5 - 40.0, f64::from(y) + 0.5 - 30.0);
            if dx.hypot(dy) <= 20.0 { 1 } else { u16::from(x >= 40) * 2 }
        });
        // Hard pixel staircases (no anti-aliasing): the strongest geometry setting.
        let options = Options { geometry: 1.0, ..Options::default() };
        let (svg, info) = to_svg(&q, None, &options, &params());
        let circle = info.guides.iter().find(|g| g.kind == GuideKind::Circle).expect("circle guide");
        assert_eq!(circle.status, crate::guide::GuideStatus::Followed, "{:?}", info.guides);
        // The red path is made of quarter-or-less arcs only (no lines, no corners).
        let red = svg.lines().find(|l| l.contains("#c80000")).expect("red path");
        assert!(!red.contains('L'), "{red}");
        assert_eq!(info.total, 0);
        // Its junctions with the boundary between the backgrounds were moved onto the circle:
        // every end point of the red path is on it (center 40,30, radius ≈ 20).
        let ends: Vec<(f64, f64)> = red
            .split(['M', 'C'])
            .filter_map(|seg| {
                let v: Vec<f64> = seg.split([' ', '"', 'Z']).filter_map(|t| t.parse().ok()).collect();
                (v.len() >= 2).then(|| (v[v.len() - 2], v[v.len() - 1]))
            })
            .collect();
        let radii: Vec<f64> = ends.iter().map(|&(x, y)| (x - 40.0).hypot(y - 30.0)).collect();
        let (lo, hi) = radii.iter().fold((f64::MAX, f64::MIN), |(l, h), &r| (l.min(r), h.max(r)));
        assert!(hi - lo < 0.05, "{radii:?}");
    }

    #[test]
    fn symmetric_shape_across_two_backgrounds_is_made_symmetric() {
        // A plus sign (arms 8 px wide, 40 px across) over a white/blue boundary, with one
        // stray pixel on its right arm: its outline has several runs.
        let q = quantized(80, 60, |x, y| {
            let (dx, dy) = (i64::from(x) - 40, i64::from(y) - 30);
            let plus = (dx.abs() < 4 && dy.abs() < 20) || (dy.abs() < 4 && dx.abs() < 20);
            if plus || (x, y) == (58, 34) { 1 } else { u16::from(y >= 30) * 2 }
        });
        let options = Options { geometry: 1.0, ..Options::default() };
        let (_, info) = to_svg(&q, None, &options, &params());
        let sym = info.guides.iter().find(|g| g.kind == GuideKind::Symmetry).expect("symmetry guide");
        assert_eq!(sym.status, crate::guide::GuideStatus::Followed, "{:?}", info.guides);
    }

    #[test]
    fn seams_are_filled_under_the_later_shape() {
        // Red and blue halves: the first drawn gets a stroke along their shared boundary only.
        let q = quantized(20, 10, |x, _| if x < 10 { 1 } else { 2 });
        let svg = to_svg(&q, None, &Options::default(), &params()).0;
        let lines: Vec<&str> = svg.lines().filter(|l| l.starts_with("<path")).collect();
        assert_eq!(lines.len(), 3, "{svg}");
        assert!(lines[1].contains("stroke=") && lines[1].contains("M10 "), "{svg}");
        // Same colour as the shape it belongs to (the one just before it).
        let fill = &lines[0][lines[0].find("fill=").expect("fill")..][6..13];
        assert!(lines[1].contains(&format!("stroke=\"{fill}")), "{svg}");
        // Off: no stroke.
        let plain = to_svg(&q, None, &Options { fill_gaps: false, ..Options::default() }, &params()).0;
        assert!(!plain.contains("stroke="), "{plain}");
    }

    #[test]
    fn transparent_hole_is_kept_when_stacked() {
        let mut q = quantized(12, 12, |x, y| u16::from(!((4..8).contains(&x) && (4..8).contains(&y))));
        for l in &mut q.labels {
            if *l == 0 {
                *l = TRANSPARENT;
            }
        }
        let svg = svg(&q, Stacking::Stacked);
        assert_eq!(svg.matches('M').count(), 2, "{svg}");
    }
}
