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
    pub fn len(self) -> f64 {
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
}

/// How one corner is drawn.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CornerStyle {
    /// Through its apex, with the tangents of its two sides.
    Sharp,
    /// No constraint: the curve follows the pixels (best-fit rounding).
    Free,
    /// A circular arc of this radius (px), tangent to both sides.
    Arc(f64),
}

const RDP_EPSILON: f64 = 0.8;
const SMOOTHING_PASSES: usize = 3;
/// Points on each side used to estimate a tangent.
const TANGENT_WINDOW: usize = 4;
/// Simplified-polygon vertices turning more than this are corner candidates.
const CANDIDATE_TURN_DEG: f64 = 25.0;
/// A candidate farther than this from its theoretical apex is not a corner.
const MAX_APEX_DISTANCE: f64 = 4.0;
/// Polyline points needed on each side of a corner to measure its sides.
const MIN_SIDE_POINTS: usize = 3;

/// A corner candidate of a run.
#[derive(Debug, Clone, Copy)]
pub struct Corner {
    /// Index of the polyline point nearest to the corner.
    index: usize,
    /// Intersection of the two neighboring sides (where a sharp corner would be).
    pub apex: P,
    /// Unit directions of travel along the incoming and outgoing sides.
    din: P,
    dout: P,
    /// Distance from the apex to the traced boundary: ~0.35 px for a pixel-sharp corner,
    /// r(1/sin(α/2) − 1) for a corner rounded with radius r.
    pub gap: f64,
    /// Turn angle (radians): 0 = straight, π = U-turn.
    pub turn: f64,
    /// Neighboring simplified-polygon vertices (limits of the corner zone).
    prev: usize,
    next: usize,
}

/// A run ready to fit: its boundary polyline and its corner candidates.
#[derive(Debug, Clone)]
pub struct Prepared {
    pts: Vec<P>,
    pub closed: bool,
    pub corners: Vec<Corner>,
}

impl Prepared {
    /// (best-fit radius, largest radius) of corner `c`, in px. The best fit comes from the
    /// gap; the largest radius keeps the tangent points on the first half of each side (the
    /// neighboring corner may round the other half).
    pub fn radii(&self, c: usize) -> (f64, f64) {
        let c = &self.corners[c];
        let half = ((std::f64::consts::PI - c.turn) / 2.0).max(1e-3); // half interior angle
        let r_fit = c.gap.max(0.0) / (1.0 / half.sin() - 1.0).max(1e-6);
        let side = |i: usize| (self.pts[i] - c.apex).len();
        let r_max = (side(c.prev).min(side(c.next)) * 0.5 * half.tan()).max(0.0);
        (r_fit.min(r_max), r_max)
    }

    /// Radius for the relative setting `rel` (−1 = sharp, 0 = best fit, 1 = largest).
    /// Quadratic above best fit: small slider values give small, controllable radii.
    pub fn relative_radius(&self, c: usize, rel: f64) -> f64 {
        let (r_fit, r_max) = self.radii(c);
        let r = if rel < 0.0 { r_fit * (1.0 + rel) } else { r_fit + rel * rel * (r_max - r_fit) };
        r.clamp(0.0, r_max)
    }
}

/// Build the polyline of a run from its grid vertices and one boundary point per crack edge
/// (`mids[i]` lies on the edge from `vertices[i]` to the next vertex), and find its corners.
/// `ends` are the positions of the two end junctions of an open run (ignored when closed).
pub fn prepare(vertices: &[(i64, i64)], mids: &[P], ends: (P, P), closed: bool) -> Prepared {
    let pts: Vec<P> = if closed {
        mids.to_vec()
    } else {
        let k = vertices.len() - 1;
        let mut pts = vec![ends.0];
        pts.extend_from_slice(&mids[..k]);
        pts.push(ends.1);
        pts
    };
    let n = pts.len();
    let keep = if closed {
        // Start the simplification on extreme points (the point farthest from an arbitrary one,
        // then the point farthest from it): on a polygon they are true vertices, so no corner
        // is cut in two by the start of the loop.
        let farthest = |from: usize| {
            (0..n)
                .max_by(|&a, &b| (pts[a] - pts[from]).len().total_cmp(&(pts[b] - pts[from]).len()))
                .unwrap_or(0)
        };
        let start = farthest(0);
        let rot: Vec<P> = (0..=n).map(|i| pts[(start + i) % n]).collect();
        let far = farthest(start);
        let far = (far + n - start) % n;
        let mut keep = vec![0, far];
        if far > 0 {
            rdp(&rot, 0, far, &mut keep);
            let mut tail = Vec::new();
            rdp(&rot, far, n, &mut tail);
            keep.extend(tail.into_iter().filter(|&i| i < n));
        }
        keep.into_iter().map(|i| (i + start) % n).collect()
    } else {
        let mut keep = vec![0, n - 1];
        rdp(&pts, 0, n - 1, &mut keep);
        keep
    };
    let mut keep = keep;
    keep.sort_unstable();
    keep.dedup();
    let keep = merge_close_vertices(&keep, n, closed);
    let corners = find_corners(&pts, &keep, closed);
    Prepared { pts, closed, corners }
}

/// A sharp tip a few pixels wide leaves 2 or 3 simplified-polygon vertices a couple of points
/// apart: merge such clusters into one vertex (their middle), so the tip is one corner.
/// Open runs keep their end points.
fn merge_close_vertices(keep: &[usize], n: usize, closed: bool) -> Vec<usize> {
    let gap = |a: usize, b: usize| if b >= a { b - a } else { b + n - a };
    let mut clusters: Vec<Vec<usize>> = Vec::new();
    for &k in keep {
        match clusters.last_mut() {
            Some(c) if gap(*c.last().unwrap_or(&k), k) < MIN_SIDE_POINTS => c.push(k),
            _ => clusters.push(vec![k]),
        }
    }
    // Closed runs: the last cluster may continue into the first one.
    if closed && clusters.len() > 1 {
        let (first, last) = (clusters[0][0], *clusters[clusters.len() - 1].last().unwrap_or(&0));
        if gap(last, first) < MIN_SIDE_POINTS {
            let tail = clusters.pop().unwrap_or_default();
            clusters[0].splice(0..0, tail);
        }
    }
    let mut out: Vec<usize> = clusters
        .iter()
        .map(|c| {
            if !closed && c.contains(&0) {
                0
            } else if !closed && c.contains(&(n - 1)) {
                n - 1
            } else {
                // Middle of the cluster, following the loop order.
                (c[0] + gap(c[0], c[c.len() - 1]) / 2) % n
            }
        })
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn find_corners(pts: &[P], keep: &[usize], closed: bool) -> Vec<Corner> {
    let m = keep.len();
    let n = pts.len();
    if m < 3 && closed {
        return Vec::new();
    }
    let at = |i: isize| pts[i.rem_euclid(n as isize) as usize];
    let range = if closed { 0..m } else { 1..m.saturating_sub(1) };
    let mut out = Vec::new();
    for j in range {
        let (prev, c, next) = (keep[(j + m - 1) % m], keep[j], keep[(j + 1) % m]);
        // Signed index offsets (closed runs wrap around).
        let back = if prev < c { c - prev } else { c + n - prev };
        let fwd = if next > c { next - c } else { next + n - c };
        if back < MIN_SIDE_POINTS || fwd < MIN_SIDE_POINTS {
            continue; // sides too short to measure: a pixel step, not a corner
        }
        let ci = c as isize;
        // Fit the two sides away from the corner itself (skip 2 points next to it).
        let side = |from: isize, to: isize| -> Option<(P, P)> {
            let lo = from.min(to);
            let hi = from.max(to);
            let pts: Vec<P> = (lo..=hi).map(at).collect();
            fit_line(&pts)
        };
        let skip = 2isize;
        // Each side is fitted away from both of its ends (the neighboring corner bends too).
        let (b, f) = (back as isize, fwd as isize);
        let trim = |len: isize| if len > 2 * skip + 1 { skip } else { 0 };
        let (Some(lin), Some(lout)) = (
            side(ci - b + trim(b), (ci - trim(b)).max(ci - b + 1)),
            side((ci + trim(f)).min(ci + f - 1), ci + f - trim(f)),
        ) else {
            continue;
        };
        let orient = |d: P, from: P, to: P| if d.dot(to - from) < 0.0 { -d } else { d };
        let din = orient(lin.1, at(ci - back as isize), at(ci));
        let dout = orient(lout.1, at(ci), at(ci + fwd as isize));
        let turn = din.dot(dout).clamp(-1.0, 1.0).acos();
        if turn < CANDIDATE_TURN_DEG.to_radians() {
            continue;
        }
        let Some(apex) = intersect(lin.0, din, lout.0, dout) else { continue };
        if (apex - at(ci)).len() > MAX_APEX_DISTANCE {
            continue;
        }
        let gap = (-3..3).map(|k| dist_to_segment(apex, at(ci + k), at(ci + k + 1))).fold(f64::MAX, f64::min);
        out.push(Corner { index: c, apex, din, dout, gap, turn, prev, next });
    }
    out
}

/// Total least-squares line through `pts`: (centroid, unit direction).
fn fit_line(pts: &[P]) -> Option<(P, P)> {
    if pts.len() < 2 {
        return None;
    }
    let k = 1.0 / pts.len() as f64;
    let c = pts.iter().fold(P::default(), |a, &p| a + p) * k;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for &p in pts {
        let d = p - c;
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    Some((c, P::new(angle.cos(), angle.sin())))
}

fn intersect(p: P, d: P, q: P, e: P) -> Option<P> {
    let cross = d.x * e.y - d.y * e.x;
    if cross.abs() < 1e-3 {
        return None;
    }
    let t = ((q.x - p.x) * e.y - (q.y - p.y) * e.x) / cross;
    Some(p + d * t)
}

/// Where the curve must pass, with optional fixed tangents (direction of travel).
#[derive(Debug, Clone, Copy)]
struct Anchor {
    index: usize,
    p: P,
    t_in: Option<P>,
    t_out: Option<P>,
    /// Draw a circular arc (as a cubic) from this anchor to the next one.
    arc: Option<(P, P)>,
}

/// Fit a prepared run; `styles[i]` says how corner `i` is drawn.
pub fn fit_prepared(run: &Prepared, styles: &[CornerStyle], params: FitParams) -> Curve {
    let n = run.pts.len();
    let mut anchors: Vec<Anchor> = Vec::new();
    for (corner, &style) in run.corners.iter().zip(styles) {
        match style {
            CornerStyle::Sharp => anchors.push(Anchor {
                index: corner.index,
                p: corner.apex,
                t_in: Some(corner.din),
                t_out: Some(corner.dout),
                arc: None,
            }),
            CornerStyle::Arc(r) if r > 1e-3 => anchors.extend(arc_anchors(run, corner, r)),
            CornerStyle::Arc(_) | CornerStyle::Free => {}
        }
    }
    let mut pts = run.pts.clone();
    if !run.closed {
        let first = Anchor { index: 0, p: pts[0], t_in: None, t_out: None, arc: None };
        let last = Anchor { index: n - 1, p: pts[n - 1], t_in: None, t_out: None, arc: None };
        anchors.insert(0, first);
        anchors.push(last);
    }
    anchors.sort_by_key(|a| a.index);
    anchors.dedup_by_key(|a| a.index);
    if anchors.is_empty() {
        // Smooth closed loop: one span starting at point 0 with a central tangent.
        smooth(&mut pts, true);
        let mut loop_pts = pts.clone();
        loop_pts.push(pts[0]);
        let t = (pts[1 % n] - pts[n - 1]).normalized();
        let mut segs = Vec::new();
        fit_cubic(&loop_pts, t, -t, params.tolerance, &mut segs, 0);
        return Curve { start: pts[0], segs };
    }
    let mut segs = Vec::new();
    let count = if run.closed { anchors.len() } else { anchors.len() - 1 };
    for k in 0..count {
        let a = anchors[k];
        let b = anchors[(k + 1) % anchors.len()];
        if let Some((c1, c2)) = a.arc {
            segs.push(Seg::Cubic(c1, c2, b.p));
            continue;
        }
        let mut span = vec![a.p];
        let mut i = (a.index + 1) % n;
        while i != b.index {
            span.push(pts[i]);
            i = (i + 1) % n;
        }
        span.push(b.p);
        // Drop the data points of the corner zones: the leading points still behind anchor `a`
        // and the trailing points already past anchor `b` (only next to the anchors: a curve
        // that wraps around passes "behind" them again further on).
        let len = span.len();
        let mut inner = &span[1..len - 1];
        if let Some(t) = a.t_out {
            let k = inner.iter().take_while(|&&p| (p - a.p).dot(t) <= 0.0).count();
            inner = &inner[k..];
        }
        if let Some(t) = b.t_in {
            let k = inner.iter().rev().take_while(|&&p| (b.p - p).dot(t) <= 0.0).count();
            inner = &inner[..inner.len() - k];
        }
        let inner = inner.to_vec();
        let mut span = vec![a.p];
        span.extend(inner);
        span.push(b.p);
        smooth(&mut span, false);
        if span.len() < 3 && (span[0] - span[span.len() - 1]).len() < 1e-9 {
            continue;
        }
        let t1 = a.t_out.unwrap_or_else(|| end_tangent(span.iter().copied()));
        let t2 = b.t_in.map_or_else(|| end_tangent(span.iter().rev().copied()), |t| -t);
        fit_cubic(&span, t1, t2, params.tolerance, &mut segs, 0);
    }
    Curve { start: anchors[0].p, segs }
}

/// Tangent points and arc (radius `r`, already clamped by the caller) of a rounded corner.
fn arc_anchors(run: &Prepared, c: &Corner, r: f64) -> [Anchor; 2] {
    let half = ((std::f64::consts::PI - c.turn) / 2.0).max(1e-3);
    let t = r / half.tan();
    let tin = c.apex - c.din * t;
    let tout = c.apex + c.dout * t;
    // Cubic approximation of the arc: handle length 4/3·tan(turn/4)·r.
    let h = 4.0 / 3.0 * (c.turn / 4.0).tan() * r;
    let n = run.pts.len();
    let nearest = |p: P, from: usize, to: usize| {
        let mut best = (from, f64::MAX);
        let mut i = from;
        loop {
            let d = (run.pts[i] - p).len();
            if d < best.1 {
                best = (i, d);
            }
            if i == to {
                break;
            }
            i = (i + 1) % n;
        }
        best.0
    };
    let i_in = nearest(tin, c.prev, c.index);
    let i_out = nearest(tout, c.index, c.next);
    let i_out = if i_out == i_in { (i_in + 1) % n } else { i_out };
    [
        Anchor {
            index: i_in,
            p: tin,
            t_in: Some(c.din),
            t_out: Some(c.din),
            arc: Some((tin + c.din * h, tout - c.dout * h)),
        },
        Anchor { index: i_out, p: tout, t_in: Some(c.dout), t_out: Some(c.dout), arc: None },
    ]
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
        FitParams { tolerance: 0.7 }
    }

    /// Midpoints of the crack edges of a run (no sub-pixel information).
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

    /// Fit with every corner sharp.
    fn fit_sharp(v: &[(i64, i64)], closed: bool, ends: (P, P)) -> Curve {
        let run = prepare(v, &grid_mids(v, closed), ends, closed);
        fit_prepared(&run, &vec![CornerStyle::Sharp; run.corners.len()], params())
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
        let c = fit_sharp(&square(10), true, (P::default(), P::default()));
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
        let c = fit_sharp(&v, false, (P::new(0.0, 0.0), P::new(8.0, 2.0)));
        assert_eq!(c.start, P::new(0.0, 0.0));
        assert_eq!(c.segs.last().map(|s| s.end()), Some(P::new(8.0, 2.0)));
        let r = c.reversed();
        assert_eq!(r.start, P::new(8.0, 2.0));
        assert_eq!(r.segs.last().map(|s| s.end()), Some(P::new(0.0, 0.0)));
        assert_eq!(r.reversed(), c);
    }
}
