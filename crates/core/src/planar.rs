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

use crate::RgbaImage;
use crate::Stacking;
use crate::fit::{Curve, FitParams, P, Seg, fit_run};
use crate::palette::{Quantized, TRANSPARENT};

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
    curves: Vec<Curve>,
}

/// Trace all regions and write the SVG document.
pub fn to_svg(q: &Quantized, image: Option<&RgbaImage>, stacking: Stacking, params: &FitParams) -> String {
    let (comp, comp_labels) = components(q);
    let grid = Grid { w: i64::from(q.width), h: i64::from(q.height), comp: &comp };
    let edge_point = EdgePoint { q, image, w: grid.w, h: grid.h };
    let mut runs = Runs { index: HashMap::new(), curves: Vec::new() };
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
                let cycle = trace_cycle(&grid, &edge_point, (x, y, d), left, &mut visited, &mut runs, params);
                cycles[left as usize].push(cycle);
            }
        }
    }

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
                write_cycle(&mut d, cycle, &runs.curves);
            }
        }
        let _ = writeln!(svg, r##"<path fill="#{r:02x}{g:02x}{b:02x}" d="{d}"/>"##);
    }
    svg.push_str("</svg>\n");
    svg
}

#[allow(clippy::too_many_lines)]
fn trace_cycle(
    grid: &Grid,
    edge_point: &EdgePoint,
    start: (i64, i64, u8),
    c: u32,
    visited: &mut [bool],
    runs: &mut Runs,
    params: &FitParams,
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
        out.push(lookup_run(grid, edge_point, &edges, true, runs, params));
    } else {
        for (k, &a) in junctions.iter().enumerate() {
            let b = junctions[(k + 1) % junctions.len()];
            let len = (b + n - a - 1) % n + 1;
            let run: Vec<_> = (0..len).map(|i| edges[(a + i) % n]).collect();
            out.push(lookup_run(grid, edge_point, &run, false, runs, params));
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
    params: &FitParams,
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
    let curve = fit_run(&vertices, &mids, closed, params);
    let idx = runs.curves.len();
    runs.curves.push(curve);
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

impl EdgePoint<'_> {
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
        let model = |l: u16| self.q.model[l as usize].map(f64::from);
        let (ca, cb) = (model(la), model(lb));
        let ab: [f64; 3] = std::array::from_fn(|k| cb[k] - ca[k]);
        let len2: f64 = ab.iter().map(|v| v * v).sum();
        if len2 < 40.0 * 40.0 {
            return None;
        }
        // Fraction of color b in a pixel (projection on the a→b segment).
        let cover_b = |c: [f64; 3]| {
            (ab.iter().zip(c.iter().zip(&ca)).map(|(d, (x, a))| d * (x - a)).sum::<f64>() / len2)
                .clamp(0.0, 1.0)
        };
        // b occupies the axis beyond e (crack at 0, l covers [-1, 0], r covers [0, 1]).
        let e = (1.0 - cover_b(color(ir))) - cover_b(color(il));
        Some(e.clamp(-MAX_SHIFT, MAX_SHIFT))
    }
}

fn fmt(v: f64, out: &mut String) {
    let r = (v * 100.0).round() / 100.0;
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
            model: vec![[255, 255, 255], [200, 0, 0], [0, 0, 200]],
            info: crate::Palette::default(),
        }
    }

    fn params() -> FitParams {
        FitParams { tolerance: 0.7, corner_cos: 60f64.to_radians().cos() }
    }

    #[test]
    fn square_in_background_cutout_and_stacked() {
        let q = quantized(20, 20, |x, y| u16::from((5..15).contains(&x) && (5..15).contains(&y)));
        let cut = to_svg(&q, None, Stacking::Cutout, &params());
        assert_eq!(cut.matches("<path").count(), 2);
        // Background has an outer boundary and a hole in cutout mode.
        assert_eq!(cut.matches('M').count(), 3, "{cut}");
        let stacked = to_svg(&q, None, Stacking::Stacked, &params());
        assert_eq!(stacked.matches('M').count(), 2, "{stacked}");
        assert!(stacked.find("#ffffff") < stacked.find("#c80000"), "{stacked}");
        assert!(stacked.contains("M5 5") || stacked.contains("L5 5"), "{stacked}");
    }

    #[test]
    fn neighbors_share_the_same_boundary() {
        // Three vertical bands: the two red/blue boundary points must appear in both paths.
        let q = quantized(30, 10, |x, _| (x / 10) as u16);
        let svg = to_svg(&q, None, Stacking::Cutout, &params());
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
    fn transparent_hole_is_kept_when_stacked() {
        let mut q = quantized(12, 12, |x, y| u16::from(!((4..8).contains(&x) && (4..8).contains(&y))));
        for l in &mut q.labels {
            if *l == 0 {
                *l = TRANSPARENT;
            }
        }
        let svg = to_svg(&q, None, Stacking::Stacked, &params());
        assert_eq!(svg.matches('M').count(), 2, "{svg}");
    }
}
