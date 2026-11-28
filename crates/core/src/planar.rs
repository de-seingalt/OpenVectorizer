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
use crate::fit::{CornerStyle, Curve, FitParams, P, Prepared, Seg, fit_prepared, prepare};
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

struct Runs {
    index: HashMap<u64, usize>,
    prepared: Vec<Prepared>,
    /// Replace closed boundaries by circles and ellipses.
    shapes: bool,
}

/// Corner candidates of the whole image and how many are sharp.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CornerStats {
    pub total: u32,
    pub sharp: u32,
    /// Every corner, for the editor (in no particular order).
    pub list: Vec<CornerInfo>,
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
}

/// In automatic mode, a corner is sharp when the boundary passes this close to its apex...
const AUTO_SHARP_GAP: f64 = 0.75;
/// ... or when the radius that would round it is this small (acute tips: anti-aliasing cuts
/// their point, so their gap is large even when they are sharp).
const AUTO_SHARP_RADIUS: f64 = 0.5;
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
            });
        }
    }
    let count = list.iter().filter(|k| k.sharp).count() as u32;
    (styles, CornerStats { total: list.len() as u32, sharp: count, list })
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
                while j < order.len()
                    && run.corners[order[j]].turn - run.corners[order[j - 1]].turn < GROUP_TURN
                {
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
    let stacking = options.stacking;
    let (comp, comp_labels) = components(q);
    let grid = Grid { w: i64::from(q.width), h: i64::from(q.height), comp: &comp };
    let edge_point = EdgePoint { q, image, w: grid.w, h: grid.h };
    let mut runs = Runs { index: HashMap::new(), prepared: Vec::new(), shapes: options.shapes };
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

    let (styles, corner_stats) = decide_corners(&runs.prepared, options);
    let curves: Vec<Curve> =
        runs.prepared.iter().zip(&styles).map(|(run, styles)| fit_prepared(run, styles, *params)).collect();

    // Draw order: largest outline first.
    let outer_area = |c: &[Cycle]| c.iter().map(|k| -k.area).fold(0.0, f64::max);
    let mut order: Vec<usize> = (0..cycles.len()).filter(|&c| !cycles[c].is_empty()).collect();
    order.sort_by(|&a, &b| outer_area(&cycles[b]).total_cmp(&outer_area(&cycles[a])));

    let has_transparency = q.labels.contains(&TRANSPARENT);
    let mut stamp = vec![u32::MAX; comp.len()];
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
        for cycle in &cycles[c] {
            let is_hole = cycle.area > 0.0;
            let keep = match stacking {
                Stacking::Cutout => true,
                Stacking::Stacked => {
                    !is_hole
                        || (has_transparency
                            && hole_has_transparency(&grid, c as u32, cycle.outside_pixel, &mut stamp))
                }
            };
            if keep {
                write_cycle(&mut d, cycle, &curves);
            }
        }
        let _ = writeln!(svg, r##"<path fill="#{r:02x}{g:02x}{b:02x}" d="{d}"/>"##);
    }
    svg.push_str("</svg>\n");
    (svg, corner_stats)
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
    let idx = runs.prepared.len();
    // An isolated shape that is nearly symmetric is made exactly symmetric.
    // Boundaries along the image frame keep their straight frame edges as they are.
    let on_border = vertices.iter().any(|&(x, y)| x == 0 || y == 0 || x == grid.w || y == grid.h);
    let found = if closed && runs.shapes && !on_border { crate::shape::symmetrize(&mids) } else { None };
    let (mids, symmetry) = match found {
        Some((sym, kind)) => (sym, Some(kind)),
        None => (mids, None),
    };
    let mut prepared = prepare(&vertices, &mids, ends, closed, symmetry);
    if runs.shapes {
        prepared.detect_shape();
    }
    // Corners on the image frame are not shape corners: always sharp, never counted.
    let (w, h) = (grid.w as f64, grid.h as f64);
    let on_frame = |p: P| p.x <= 0.01 || p.y <= 0.01 || p.x >= w - 0.01 || p.y >= h - 0.01;
    let (frame, corners) = prepared.corners.iter().partition(|c| on_frame(c.apex));
    (prepared.frame, prepared.corners) = (frame, corners);
    runs.prepared.push(prepared);
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
        FitParams { tolerance: 0.7 }
    }

    fn svg(q: &Quantized, stacking: Stacking) -> String {
        to_svg(q, None, &Options { stacking, ..Options::default() }, &params()).0
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
