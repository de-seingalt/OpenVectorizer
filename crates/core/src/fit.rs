//! Curve fitting for one boundary run (a chain of pixel crack edges between two junctions,
//! or a closed loop).
//!
//! 1. The staircase is replaced by the midpoints of its unit edges (a 45° staircase becomes a
//!    straight line); junction endpoints stay exactly on their grid vertex.
//! 2. Corners: vertices of a Ramer–Douglas–Peucker simplification whose turn exceeds the
//!    threshold. A corner is snapped to the grid vertex that sharpens it.
//! 3. Between corners, cubic Béziers are fitted with Schneider's algorithm (least squares with
//!    fixed end tangents, Newton reparametrization, split at the worst point); spans that are
//!    straight within tolerance become lines.

use std::ops::{Add, Mul, Neg, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

impl P {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    fn dot(self, o: P) -> f64 {
        self.x * o.x + self.y * o.y
    }
    fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn normalized(self) -> P {
        let l = self.len();
        if l < 1e-12 { P::default() } else { self * (1.0 / l) }
    }
}

impl Add for P {
    type Output = P;
    fn add(self, o: P) -> P {
        P::new(self.x + o.x, self.y + o.y)
    }
}
impl Sub for P {
    type Output = P;
    fn sub(self, o: P) -> P {
        P::new(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f64> for P {
    type Output = P;
    fn mul(self, k: f64) -> P {
        P::new(self.x * k, self.y * k)
    }
}
impl Neg for P {
    type Output = P;
    fn neg(self) -> P {
        P::new(-self.x, -self.y)
    }
}

/// One segment, starting where the previous one ends.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Seg {
    Line(P),
    Cubic(P, P, P),
}

impl Seg {
    pub fn end(self) -> P {
        match self {
            Seg::Line(e) | Seg::Cubic(_, _, e) => e,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub start: P,
    pub segs: Vec<Seg>,
}

impl Curve {
    pub fn reversed(&self) -> Curve {
        let mut starts = vec![self.start];
        starts.extend(self.segs.iter().map(|s| s.end()));
        let start = *starts.last().unwrap_or(&self.start);
        let segs = self
            .segs
            .iter()
            .enumerate()
            .rev()
            .map(|(i, s)| match *s {
                Seg::Line(_) => Seg::Line(starts[i]),
                Seg::Cubic(c1, c2, _) => Seg::Cubic(c2, c1, starts[i]),
            })
            .collect();
        Curve { start, segs }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FitParams {
    /// Max distance (px) between the fitted curve and the edge midpoints.
    pub tolerance: f64,
    /// A polygon vertex is a corner when the cosine of its turn angle is below this.
    pub corner_cos: f64,
}

const RDP_EPSILON: f64 = 0.8;
const SMOOTHING_PASSES: usize = 3;
/// Points on each side used to estimate a tangent.
const TANGENT_WINDOW: usize = 4;

/// Fit a run given by its grid vertices and one boundary point per crack edge (`mids[i]` lies on
/// the edge from `vertices[i]` to the next vertex: its midpoint, or a sub-pixel estimate of
/// the color transition). Open runs keep their end vertices exactly.
///
/// `ends` are the positions of the two end junctions of an open run (ignored when closed).
pub fn fit_run(vertices: &[(i64, i64)], mids: &[P], ends: (P, P), closed: bool, params: &FitParams) -> Curve {
    let v = |i: usize| P::new(vertices[i].0 as f64, vertices[i].1 as f64);
    if closed {
        let n = vertices.len();
        let ends: Vec<(P, P)> = (0..n).map(|i| (v(i), v((i + 1) % n))).collect();
        fit_closed(mids.to_vec(), &ends, params)
    } else {
        let k = vertices.len() - 1;
        let mut pts = vec![ends.0];
        pts.extend_from_slice(&mids[..k]);
        pts.push(ends.1);
        let ends_of_edges = {
            let mut e = vec![(ends.0, ends.0)];
            e.extend((1..=k).map(|i| (v(i - 1), v(i))));
            e.push((ends.1, ends.1));
            e
        };
        let ends = ends_of_edges;
        let mut keep = vec![0, pts.len() - 1];
        rdp(&pts, 0, pts.len() - 1, &mut keep);
        keep.sort_unstable();
        let corners = corners_of(&pts, &keep, false, params.corner_cos);
        snap(&mut pts, &ends, &keep, &corners, false);
        let mut breaks = vec![0];
        breaks.extend(corners);
        breaks.push(pts.len() - 1);
        breaks.dedup();
        fit_spans(&pts, &breaks, params)
    }
}

/// Midpoints of the crack edges of a run (no sub-pixel information).
#[cfg(test)]
fn grid_mids(vertices: &[(i64, i64)], closed: bool) -> Vec<P> {
    let n = vertices.len();
    let edges = if closed { n } else { n - 1 };
    (0..edges)
        .map(|i| {
            let (a, b) = (vertices[i], vertices[(i + 1) % n]);
            P::new((a.0 + b.0) as f64 * 0.5, (a.1 + b.1) as f64 * 0.5)
        })
        .collect()
}

fn fit_closed(pts: Vec<P>, ends: &[(P, P)], params: &FitParams) -> Curve {
    let n = pts.len();
    let far =
        (1..n).max_by(|&a, &b| (pts[a] - pts[0]).len().total_cmp(&(pts[b] - pts[0]).len())).unwrap_or(0);
    let mut keep = vec![0, far];
    if far > 0 {
        rdp(&pts, 0, far, &mut keep);
        let mut ext = pts.clone();
        ext.push(pts[0]);
        let mut tail = Vec::new();
        rdp(&ext, far, n, &mut tail);
        keep.extend(tail.into_iter().filter(|&i| i < n));
    }
    keep.sort_unstable();
    keep.dedup();
    let corners = corners_of(&pts, &keep, true, params.corner_cos);
    let mut pts = pts;
    if corners.is_empty() {
        // Smooth loop: one closed span starting at point 0 with a central tangent.
        let mut pts = pts;
        smooth(&mut pts, true);
        let mut loop_pts = pts.clone();
        loop_pts.push(pts[0]);
        let t = (pts[1 % n] - pts[n - 1]).normalized();
        let mut segs = Vec::new();
        fit_cubic(&loop_pts, t, -t, params.tolerance, &mut segs, 0);
        return Curve { start: pts[0], segs };
    }
    snap(&mut pts, ends, &keep, &corners, true);
    // Rotate so the first corner is index 0, then close the polyline on it.
    let c0 = corners[0];
    let mut rot: Vec<P> = pts[c0..].iter().chain(&pts[..c0]).copied().collect();
    rot.push(rot[0]);
    let mut breaks: Vec<usize> = corners.iter().map(|&c| (c + n - c0) % n).collect();
    breaks.push(n);
    fit_spans(&rot, &breaks, params)
}

/// Ramer–Douglas–Peucker: push the indices kept strictly between `a` and `b`.
fn rdp(pts: &[P], a: usize, b: usize, keep: &mut Vec<usize>) {
    if b <= a + 1 {
        return;
    }
    let (pa, pb) = (pts[a], pts[b]);
    let (i, d) = ((a + 1)..b)
        .map(|i| (i, dist_to_segment(pts[i], pa, pb)))
        .max_by(|x, y| x.1.total_cmp(&y.1))
        .unwrap_or((a, 0.0));
    if d > RDP_EPSILON {
        keep.push(i);
        rdp(pts, a, i, keep);
        rdp(pts, i, b, keep);
    }
}

fn dist_to_segment(p: P, a: P, b: P) -> f64 {
    let ab = b - a;
    let l2 = ab.dot(ab);
    if l2 < 1e-12 {
        return (p - a).len();
    }
    let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
    (p - (a + ab * t)).len()
}

/// Indices (into `pts`) of the simplified-polygon vertices that turn sharply.
fn corners_of(pts: &[P], keep: &[usize], closed: bool, corner_cos: f64) -> Vec<usize> {
    let m = keep.len();
    let range = if closed { 0..m } else { 1..m.saturating_sub(1) };
    range
        .filter(|&j| {
            if closed && m < 3 {
                return false;
            }
            let prev = pts[keep[(j + m - 1) % m]];
            let next = pts[keep[(j + 1) % m]];
            let cur = pts[keep[j]];
            let din = (cur - prev).normalized();
            let dout = (next - cur).normalized();
            din.dot(dout) < corner_cos
        })
        .map(|j| keep[j])
        .collect()
}

/// Move each corner from its edge midpoint to the edge end that makes the corner sharper.
fn snap(pts: &mut [P], ends: &[(P, P)], keep: &[usize], corners: &[usize], closed: bool) {
    let m = keep.len();
    for &c in corners {
        let Some(j) = keep.iter().position(|&k| k == c) else { continue };
        let prev = pts[keep[if closed { (j + m - 1) % m } else { j.saturating_sub(1) }]];
        let next = pts[keep[if closed { (j + 1) % m } else { (j + 1).min(m - 1) }]];
        let (a, b) = ends[c];
        pts[c] = if dist_to_segment(a, prev, next) >= dist_to_segment(b, prev, next) { a } else { b };
    }
}

fn fit_spans(pts: &[P], breaks: &[usize], params: &FitParams) -> Curve {
    let mut segs = Vec::new();
    for w in breaks.windows(2) {
        let mut span = pts[w[0]..=w[1]].to_vec();
        smooth(&mut span, false);
        let span = &span[..];
        if span.len() < 2 || (span[0] - span[span.len() - 1]).len() < 1e-9 && span.len() < 3 {
            continue;
        }
        let t1 = end_tangent(span.iter().copied());
        let t2 = end_tangent(span.iter().rev().copied());
        fit_cubic(span, t1, t2, params.tolerance, &mut segs, 0);
    }
    Curve { start: pts[0], segs }
}

/// Pixel staircases leave ±0.5 px ripples on the edge midpoints: low-pass them with a few
/// passes of the [1 2 1]/4 kernel. Open spans keep their end points (corners, junctions).
fn smooth(pts: &mut [P], closed: bool) {
    let n = pts.len();
    if n < 3 {
        return;
    }
    for _ in 0..SMOOTHING_PASSES {
        let prev = pts.to_vec();
        for i in 0..n {
            if !closed && (i == 0 || i == n - 1) {
                continue;
            }
            let (a, b) = (prev[(i + n - 1) % n], prev[(i + 1) % n]);
            pts[i] = (a + prev[i] * 2.0 + b) * 0.25;
        }
    }
}

/// Unit tangent at the first point of `it`, averaged over the next few points.
fn end_tangent(mut it: impl Iterator<Item = P>) -> P {
    let Some(p0) = it.next() else { return P::default() };
    let mut sum = P::default();
    for p in it.take(TANGENT_WINDOW) {
        sum = sum + (p - p0).normalized();
    }
    sum.normalized()
}

fn bezier(b: &[P; 4], u: f64) -> P {
    let v = 1.0 - u;
    b[0] * (v * v * v) + b[1] * (3.0 * v * v * u) + b[2] * (3.0 * v * u * u) + b[3] * (u * u * u)
}

fn fit_cubic(pts: &[P], t1: P, t2: P, tol: f64, out: &mut Vec<Seg>, depth: u32) {
    let n = pts.len();
    let (p0, p3) = (pts[0], pts[n - 1]);
    if n <= 2 {
        out.push(Seg::Line(p3));
        return;
    }
    // Only a whole span (between corners) may become a line: inside a curve, a line facet
    // would break the tangent continuity.
    let straight = pts[1..n - 1].iter().map(|&p| dist_to_segment(p, p0, p3)).fold(0.0, f64::max);
    if depth == 0 && straight <= tol * 0.5 && (p3 - p0).len() > 1e-9 {
        out.push(Seg::Line(p3));
        return;
    }
    let mut u = chord_params(pts);
    let mut bez = generate(pts, &u, t1, t2);
    let (mut err, mut split) = max_error(pts, &bez, &u);
    if err < tol {
        out.push(Seg::Cubic(bez[1], bez[2], bez[3]));
        return;
    }
    if err < tol * 4.0 {
        for _ in 0..4 {
            u = reparametrize(pts, &u, &bez);
            bez = generate(pts, &u, t1, t2);
            (err, split) = max_error(pts, &bez, &u);
            if err < tol {
                out.push(Seg::Cubic(bez[1], bez[2], bez[3]));
                return;
            }
        }
    }
    if n <= 3 || depth > 24 {
        out.extend(pts[1..].iter().map(|&p| Seg::Line(p)));
        return;
    }
    let split = split.clamp(1, n - 2);
    // Tangent at the split from a wider window: two neighbors alone are too noisy.
    let k = TANGENT_WINDOW.min(split).min(n - 1 - split);
    let tc = (pts[split - k] - pts[split + k]).normalized();
    fit_cubic(&pts[..=split], t1, tc, tol, out, depth + 1);
    fit_cubic(&pts[split..], -tc, t2, tol, out, depth + 1);
}

fn chord_params(pts: &[P]) -> Vec<f64> {
    let mut u = vec![0.0; pts.len()];
    for i in 1..pts.len() {
        u[i] = u[i - 1] + (pts[i] - pts[i - 1]).len();
    }
    let total = u[pts.len() - 1].max(1e-12);
    for x in &mut u {
        *x /= total;
    }
    u
}

fn generate(pts: &[P], u: &[f64], t1: P, t2: P) -> [P; 4] {
    let (p0, p3) = (pts[0], pts[pts.len() - 1]);
    let (mut c00, mut c01, mut c11, mut x0, mut x1) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for (&p, &t) in pts.iter().zip(u) {
        let v = 1.0 - t;
        let (b0, b1, b2, b3) = (v * v * v, 3.0 * v * v * t, 3.0 * v * t * t, t * t * t);
        let (a0, a1) = (t1 * b1, t2 * b2);
        c00 += a0.dot(a0);
        c01 += a0.dot(a1);
        c11 += a1.dot(a1);
        let tmp = p - (p0 * (b0 + b1) + p3 * (b2 + b3));
        x0 += a0.dot(tmp);
        x1 += a1.dot(tmp);
    }
    let det = c00 * c11 - c01 * c01;
    let seg = (p3 - p0).len();
    let eps = 1e-6 * seg;
    let (mut a1, mut a2) = if det.abs() < 1e-12 {
        (0.0, 0.0)
    } else {
        ((x0 * c11 - x1 * c01) / det, (c00 * x1 - c01 * x0) / det)
    };
    if a1 < eps || a2 < eps || a1 > seg * 3.0 || a2 > seg * 3.0 {
        (a1, a2) = (seg / 3.0, seg / 3.0);
    }
    [p0, p0 + t1 * a1, p3 + t2 * a2, p3]
}

fn max_error(pts: &[P], b: &[P; 4], u: &[f64]) -> (f64, usize) {
    let mut best = (0.0, pts.len() / 2);
    for i in 1..pts.len() - 1 {
        let d = (bezier(b, u[i]) - pts[i]).len();
        if d > best.0 {
            best = (d, i);
        }
    }
    best
}

fn reparametrize(pts: &[P], u: &[f64], b: &[P; 4]) -> Vec<f64> {
    let d1 = [(b[1] - b[0]) * 3.0, (b[2] - b[1]) * 3.0, (b[3] - b[2]) * 3.0];
    let d2 = [(d1[1] - d1[0]) * 2.0, (d1[2] - d1[1]) * 2.0];
    pts.iter()
        .zip(u)
        .map(|(&p, &t)| {
            let v = 1.0 - t;
            let q = bezier(b, t) - p;
            let q1 = d1[0] * (v * v) + d1[1] * (2.0 * v * t) + d1[2] * (t * t);
            let q2 = d2[0] * v + d2[1] * t;
            let den = q1.dot(q1) + q.dot(q2);
            if den.abs() < 1e-12 { t } else { (t - q.dot(q1) / den).clamp(0.0, 1.0) }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params() -> FitParams {
        FitParams { tolerance: 0.7, corner_cos: 60f64.to_radians().cos() }
    }

    fn square(n: i64) -> Vec<(i64, i64)> {
        let mut v = Vec::new();
        v.extend((0..n).map(|i| (i, 0)));
        v.extend((0..n).map(|i| (n, i)));
        v.extend((0..n).map(|i| (n - i, n)));
        v.extend((0..n).map(|i| (0, n - i)));
        v
    }

    #[test]
    fn square_keeps_four_sharp_corners() {
        let sq = square(10);
        let c = fit_run(&sq, &grid_mids(&sq, true), (P::default(), P::default()), true, &params());
        assert_eq!(c.segs.len(), 4, "{c:?}");
        assert!(c.segs.iter().all(|s| matches!(s, Seg::Line(_))));
        let mut corners: Vec<(i64, i64)> =
            c.segs.iter().map(|s| (s.end().x.round() as i64, s.end().y.round() as i64)).collect();
        corners.sort_unstable();
        assert_eq!(corners, vec![(0, 0), (0, 10), (10, 0), (10, 10)]);
    }

    #[test]
    fn open_run_keeps_endpoints_and_reverses() {
        let v: Vec<(i64, i64)> = (0..=8).map(|i| (i, i / 3)).collect();
        let c = fit_run(&v, &grid_mids(&v, false), (P::new(0.0, 0.0), P::new(8.0, 2.0)), false, &params());
        assert_eq!(c.start, P::new(0.0, 0.0));
        assert_eq!(c.segs.last().map(|s| s.end()), Some(P::new(8.0, 2.0)));
        let r = c.reversed();
        assert_eq!(r.start, P::new(8.0, 2.0));
        assert_eq!(r.segs.last().map(|s| s.end()), Some(P::new(0.0, 0.0)));
        assert_eq!(r.reversed(), c);
    }
}
