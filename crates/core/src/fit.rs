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

use crate::guide::{CANDIDATE_GEOMETRY, GuideKind, Guides};
use crate::num::math::{acos, atan2, cos, sin, sin_cos, tan};

/// Arc tolerance (px) for the geometry setting g: 0 → no arcs; 0.5 → 0.5 px; 1 → 0.8 px.
pub fn arc_tolerance(g: f64) -> f64 {
    if g > 0.0 { 0.2 + 0.6 * g } else { 0.0 }
}

/// Largest turn (degrees) of a rounding that becomes a corner.
const MAX_FILLET_CORNER_TURN: f64 = 135.0;

/// Arc tolerance (px) used to look for candidate fillets.
const CANDIDATE_ARC_TOL: f64 = 1.1;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct P {
    pub x: f64,
    pub y: f64,
}

impl P {
    pub fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
    pub fn dot(self, o: P) -> f64 {
        self.x * o.x + self.y * o.y
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn normalized(self) -> P {
        let l = self.len();
        if l < 1e-12 { P::default() } else { self * (1.0 / l) }
    }
    /// Unit vector, or +x for a null vector.
    pub fn normalized_or_x(self) -> P {
        let l = self.len();
        if l < 1e-12 { P::new(1.0, 0.0) } else { self * (1.0 / l) }
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
    /// Max distance (px) between a tangent arc (fillet between two straight parts) and the
    /// points it replaces; 0 = no fillets.
    pub arc_tol: f64,
    /// Plain tracing: no lines, arcs or fillets (soft boundaries).
    pub plain: bool,
    /// Extra tolerance in proportion to the size of a line, arc or shape (Scale setting).
    pub relative: f64,
}

/// Fillet radii shared across the whole image: `found` collects every fitted radius, `snap`
/// lists the radii to prefer (means of close radii), so similar fillets get the same radius.
#[derive(Debug, Clone, Default)]
pub struct Fillets {
    pub snap: Vec<f64>,
    pub found: Vec<f64>,
}

impl Fillets {
    /// Mean radius of each cluster of close radii (within 4 % or 0.3 px) that has several members.
    pub fn clusters(radii: &[f64]) -> Vec<f64> {
        let mut r = radii.to_vec();
        r.sort_by(f64::total_cmp);
        let mut out = Vec::new();
        let mut i = 0;
        while i < r.len() {
            let mut j = i + 1;
            while j < r.len() && r[j] - r[i] <= SNAP_RADIUS.max(r[i] * SNAP_RELATIVE) {
                j += 1;
            }
            if j - i >= 2 {
                out.push(r[i..j].iter().sum::<f64>() / (j - i) as f64);
            }
            i = j;
        }
        out
    }
}

/// Radii closer than this (px), or than this fraction, are made equal.
const SNAP_RADIUS: f64 = 0.3;
const SNAP_RELATIVE: f64 = 0.04;

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
    pub din: P,
    pub dout: P,
    /// Distance from the apex to the traced boundary: ~0.35 px for a pixel-sharp corner,
    /// r(1/sin(α/2) − 1) for a corner rounded with radius r.
    pub gap: f64,
    /// Turn angle (radians): 0 = straight, π = U-turn.
    pub turn: f64,
    /// Neighboring simplified-polygon vertices (limits of the corner zone).
    prev: usize,
    next: usize,
    /// Points of a rounding (first, last index): a corner made from a fillet replaces them
    /// when it is drawn sharp or with its own radius.
    zone: Option<(usize, usize)>,
}

impl Corner {
    /// Made from a rounding between two straight parts (see [`Prepared::add_fillet_corners`]).
    pub fn is_fillet(&self) -> bool {
        self.zone.is_some()
    }
}

/// A run ready to fit: its boundary polyline and its corner candidates.
#[derive(Debug, Clone)]
pub struct Prepared {
    pts: Vec<P>,
    pub closed: bool,
    pub corners: Vec<Corner>,
    /// Corners on the image frame: always sharp, not shown to the user.
    pub frame: Vec<Corner>,
    /// The exact circle or ellipse this closed boundary follows, if any.
    shape: Option<Curve>,
    /// A soft boundary (a band boundary inside a gradient, not an edge of the drawing): traced
    /// freely, no geometry is imposed on it.
    pub soft: bool,
}

impl Prepared {
    /// Largest distance (px) from a measured boundary point to `curve`, ignoring the
    /// points around corners drawn sharp or with a chosen radius (they leave the rounded pixels on
    /// purpose). Measures whether a geometric fit really follows the image.
    pub fn deviation(&self, styles: &[CornerStyle], curve: &Curve) -> f64 {
        // The curve as a fine polyline.
        let mut poly = vec![curve.start];
        let mut from = curve.start;
        for seg in &curve.segs {
            match *seg {
                Seg::Line(e) => poly.push(e),
                Seg::Cubic(c1, c2, e) => {
                    let length = (c1 - from).len() + (c2 - c1).len() + (e - c2).len();
                    let steps = length.ceil().clamp(2.0, 64.0) as usize;
                    let b = [from, c1, c2, e];
                    poly.extend((1..=steps).map(|k| bezier(&b, k as f64 / steps as f64)));
                }
            }
            from = seg.end();
        }
        let m = poly.len().saturating_sub(1);
        if m == 0 {
            return 0.0;
        }
        // A curve much longer than the boundary (a detour, the long way round a shape) is as
        // wrong as one that strays: report the excess length.
        let curve_length: f64 = poly.windows(2).map(|w| (w[1] - w[0]).len()).sum();
        let closing = if self.closed { (self.pts[0] - self.pts[self.pts.len() - 1]).len() } else { 0.0 };
        let length: f64 = self.pts.windows(2).map(|w| (w[1] - w[0]).len()).sum::<f64>() + closing;
        let detour = curve_length - 1.25 * length - 2.0;
        if detour > 0.0 {
            return detour;
        }
        let excused: Vec<(P, f64)> = self
            .corners
            .iter()
            .zip(styles)
            .filter(|(_, s)| !matches!(s, CornerStyle::Free))
            .map(|(c, _)| (c.apex, c.gap + 1.5))
            .chain(self.frame.iter().map(|c| (c.apex, c.gap + 1.5)))
            .collect();
        // Points and polyline run the same way: follow both, searching a window of segments
        // around the last match (the whole polyline for the first point).
        let seg_dist = |p: P, k: usize| dist_to_segment(p, poly[k], poly[k + 1]);
        let nearest = |p: P, ks: &mut dyn Iterator<Item = usize>| {
            ks.map(|k| (k, seg_dist(p, k))).min_by(|a, b| a.1.total_cmp(&b.1)).unwrap_or((0, f64::MAX))
        };
        let mut at = nearest(self.pts[0], &mut (0..m)).0;
        let mut worst = 0.0f64;
        for &p in &self.pts {
            // A window of segments around the last match (wrapping on a loop).
            let (lo, hi) = (at as i64 - 8, at as i64 + 48);
            let closed = self.closed;
            let mut window = (lo..hi).filter_map(|k| {
                if closed {
                    Some(k.rem_euclid(m as i64) as usize)
                } else {
                    usize::try_from(k).ok().filter(|&k| k < m)
                }
            });
            let (k, d) = nearest(p, &mut window);
            at = k;
            if d > worst && !excused.iter().any(|&(apex, r)| (p - apex).len() <= r) {
                worst = d;
            }
        }
        worst
    }

    /// Half the diagonal of the bounding box: a size for the relative tolerances.
    pub fn size(&self) -> f64 {
        let (mut lo, mut hi) = (P::new(f64::MAX, f64::MAX), P::new(f64::MIN, f64::MIN));
        for p in &self.pts {
            lo = P::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = P::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        (hi - lo).len() / 2.0
    }

    /// Move the end junctions of an open run (a junction placed onto a shape).
    pub fn set_ends(&mut self, start: P, end: P) {
        let n = self.pts.len();
        self.pts[0] = start;
        self.pts[n - 1] = end;
    }

    /// The boundary points (with the end junctions of an open run).
    pub fn pts(&self) -> &[P] {
        &self.pts
    }

    /// Turn every rounded corner between two straight parts (a fillet that follows the points
    /// within `arc_tol`, or a forced candidate) into a corner whose apex is where the two lines
    /// meet: the corners tool can then make it sharp, or set its radius, tangent to those lines.
    /// Corner candidates inside such a rounding are replaced by it.
    pub fn add_fillet_corners(&mut self, arc_tol: f64, rel: f64, guides: &Guides) {
        let n = self.pts.len();
        if self.shape.is_some() || self.soft || n < 8 {
            return;
        }
        let mut work = self.pts.clone();
        smooth(&mut work, self.closed);
        // A loop starts in the middle of its longest straight part (no rounding is cut).
        let start = if self.closed {
            find_lines(&work, rel)
                .iter()
                .max_by_key(|l| l.to - l.from)
                .map_or(0, |l| usize::midpoint(l.from, l.to))
        } else {
            0
        };
        let seq: Vec<P> = if self.closed { (0..=n).map(|i| work[(start + i) % n]).collect() } else { work };
        let at = |i: usize| (i + start) % n;
        let lines = find_lines(&seq, rel);
        let m = seq.len();
        for pair in lines.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            let a_start = if a.from == 0 { seq[0] } else { a.project(seq[a.from]) };
            let b_end = if b.to == m - 1 { seq[m - 1] } else { b.project(seq[b.to]) };
            let zone = &seq[a.to..=b.from];
            let found = fillet(zone, (a, a_start), (b, b_end), arc_tol, rel, &[]).or_else(|| {
                fillet(zone, (a, a_start), (b, b_end), CANDIDATE_ARC_TOL, rel, &[])
                    .filter(|f| guides.is_forced(GuideKind::Arc, f.middle))
            });
            let Some(f) = found.filter(|f| !guides.is_disabled(GuideKind::Arc, f.middle)) else { continue };
            let turn = acos(a.dir.dot(b.dir).clamp(-1.0, 1.0));
            let Some(apex) = intersect(a.origin, a.dir, b.origin, b.dir) else { continue };
            // Beyond MAX_FILLET_CORNER_TURN the lines are nearly parallel (the rounded end of a
            // stroke): their apex is far away, a sharp corner there would be a spike.
            if !(CANDIDATE_TURN_DEG..=MAX_FILLET_CORNER_TURN).contains(&turn.to_degrees()) {
                continue;
            }
            let half = (std::f64::consts::PI - turn) / 2.0;
            // Replace the candidates found inside the rounding (or right at its apex).
            let inside = |c: &Corner| {
                let i = (c.index + n - start) % n;
                (a.to..=b.from).contains(&i) || (c.apex - apex).len() <= 2.0
            };
            self.corners.retain(|c| !inside(c));
            self.corners.push(Corner {
                index: at(usize::midpoint(a.to, b.from)),
                apex,
                din: a.dir,
                dout: b.dir,
                gap: f.radius * (1.0 / sin(half) - 1.0),
                turn,
                prev: at(a.from),
                next: at(b.to.min(m - 2)),
                zone: Some((at(a.to), at(b.from))),
            });
        }
        self.corners.sort_by_key(|c| c.index);
    }

    /// Whether the boundary was replaced by a circle or an ellipse.
    pub fn has_shape(&self) -> bool {
        self.shape.is_some()
    }

    /// Replace a closed boundary that follows a circle or an ellipse by the exact shape (which
    /// has no corners).
    pub fn detect_shape(&mut self, geometry: f64, rel: f64, guides: &mut Guides) {
        if !self.closed || self.soft {
            return;
        }
        let strict = crate::shape::fit_round(&self.pts, geometry, rel);
        let within = strict.is_some();
        let found = strict.or_else(|| crate::shape::fit_round(&self.pts, CANDIDATE_GEOMETRY, rel));
        if let Some((ellipse, reverse)) = found
            && guides.add(ellipse.kind(), ellipse.top(), ellipse.guide(), within)
        {
            self.shape = Some(ellipse.curve(reverse));
            self.corners.clear();
        }
    }

    /// (best-fit radius, largest radius) of corner `c`, in px. The best fit comes from the
    /// gap; the largest radius keeps the tangent points on the first half of each side (the
    /// neighboring corner may round the other half).
    pub fn radii(&self, c: usize) -> (f64, f64) {
        let c = &self.corners[c];
        let half = ((std::f64::consts::PI - c.turn) / 2.0).max(1e-3); // half interior angle
        let r_fit = c.gap.max(0.0) / (1.0 / sin(half) - 1.0).max(1e-6);
        let side = |i: usize| (self.pts[i] - c.apex).len();
        let r_max = (side(c.prev).min(side(c.next)) * 0.5 * tan(half)).max(0.0);
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
///
/// `symmetry` (closed runs made symmetric by [`crate::shape::symmetrize`]): corner candidates
/// are repeated at all their symmetric positions, so symmetric corners stay alike.
pub fn prepare(
    vertices: &[(i64, i64)],
    mids: &[P],
    ends: (P, P),
    closed: bool,
    symmetry: Option<crate::shape::Symmetry>,
) -> Prepared {
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
    if let Some(sym) = symmetry {
        keep = keep.iter().flat_map(|&i| sym.images(i, n)).collect();
    }
    keep.sort_unstable();
    keep.dedup();
    let keep = merge_close_vertices(&keep, n, closed);
    let corners = find_corners(&pts, &keep, closed);
    Prepared { pts, closed, corners, frame: Vec::new(), shape: None, soft: false }
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
            let (origin, dir) = fit_line(&pts)?;
            Some((origin, snap_axis(dir, (pts[pts.len() - 1] - pts[0]).len())))
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
        let turn = acos(din.dot(dout).clamp(-1.0, 1.0));
        if turn < CANDIDATE_TURN_DEG.to_radians() {
            continue;
        }
        let Some(apex) = intersect(lin.0, din, lout.0, dout) else { continue };
        if (apex - at(ci)).len() > MAX_APEX_DISTANCE {
            continue;
        }
        let gap = (-3..3).map(|k| dist_to_segment(apex, at(ci + k), at(ci + k + 1))).fold(f64::MAX, f64::min);
        out.push(Corner { index: c, apex, din, dout, gap, turn, prev, next, zone: None });
    }
    out
}

/// A line within this drift (px over its length) of horizontal or vertical is made exactly so.
const SNAP_DRIFT: f64 = 0.3;

/// `dir`, made exactly horizontal or vertical when that moves the ends of a line of length
/// `len` by less than [`SNAP_DRIFT`].
pub(crate) fn snap_axis(dir: P, len: f64) -> P {
    if dir.y.abs() * len <= SNAP_DRIFT {
        P::new(dir.x.signum(), 0.0)
    } else if dir.x.abs() * len <= SNAP_DRIFT {
        P::new(0.0, dir.y.signum())
    } else {
        dir
    }
}

/// Total least-squares line through `pts`: (centroid, unit direction).
pub(crate) fn fit_line(pts: &[P]) -> Option<(P, P)> {
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
    let angle = 0.5 * atan2(2.0 * sxy, sxx - syy);
    Some((c, P::new(cos(angle), sin(angle))))
}

pub(crate) fn intersect(p: P, d: P, q: P, e: P) -> Option<P> {
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
pub fn fit_prepared(
    run: &Prepared,
    styles: &[CornerStyle],
    params: FitParams,
    fillets: &mut Fillets,
    guides: &mut Guides,
) -> Curve {
    if let Some(shape) = run.shape.as_ref().filter(|_| !params.plain) {
        return shape.clone();
    }
    let params = FitParams { plain: params.plain || run.soft, ..params };
    let n = run.pts.len();
    let mut anchors: Vec<Anchor> = Vec::new();
    let frame = run.frame.iter().map(|c| (c, CornerStyle::Sharp));
    for (corner, style) in run.corners.iter().zip(styles.iter().copied()).chain(frame) {
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
    // Points of the roundings that a fillet corner replaces (drawn sharp or with its radius).
    let mut hidden = vec![false; n];
    for (corner, style) in run.corners.iter().zip(styles) {
        if let (Some((from, to)), CornerStyle::Sharp | CornerStyle::Arc(_)) = (corner.zone, style) {
            let mut i = from;
            while i != to {
                hidden[i] = true;
                i = (i + 1) % n;
            }
            hidden[to] = true;
        }
    }
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
        // Start the loop in the middle of its longest straight part, if it has one: every
        // fillet between straight parts is then inside the span.
        let lines = enabled_lines(&pts, params.relative, guides, false);
        let start = lines.iter().max_by_key(|l| l.to - l.from).map_or(0, |l| usize::midpoint(l.from, l.to));
        pts.rotate_left(start);
        let mut loop_pts = pts.clone();
        loop_pts.push(pts[0]);
        let t = (pts[1 % n] - pts[n - 1]).normalized();
        let mut segs = Vec::new();
        fit_span(&loop_pts, Some(t), Some(-t), params, fillets, guides, &mut segs);
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
            if !hidden[i] {
                span.push(pts[i]);
            }
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
        fit_span(&span, a.t_out, b.t_in.map(|t| -t), params, fillets, guides, &mut segs);
    }
    Curve { start: anchors[0].p, segs }
}

/// Straight parts must stay this close to their line (px)...
const LINE_TOL: f64 = 0.3;
/// ...show no systematic bow larger than this (sagitta of the best parabola, px)...
const LINE_BOW: f64 = 0.06;
/// ...and be at least this long (px).
const MIN_LINE_LEN: f64 = 6.0;
/// Line ends are trimmed back to where the points are this close to the line: the curve that
/// follows starts where the shape really leaves the line.
const LINE_END_TOL: f64 = 0.15;
/// A straight part whose surroundings lie this close to one circle is part of an arc.
const ARC_TOL: f64 = 0.3;

/// A straight part `pts[from..=to]` of a span.
#[derive(Debug, Clone, Copy)]
struct Line {
    from: usize,
    to: usize,
    /// A point of the line and its unit direction (along the span).
    origin: P,
    dir: P,
}

impl Line {
    fn project(&self, p: P) -> P {
        self.origin + self.dir * (p - self.origin).dot(self.dir)
    }
}

/// Whether `pts` is straight: (line, deviation of each point) if so.
fn straight(pts: &[P], rel: f64) -> Option<(P, P, Vec<f64>)> {
    let (origin, dir) = fit_line(pts)?;
    let dir = if dir.dot(pts[pts.len() - 1] - pts[0]) < 0.0 { -dir } else { dir };
    let normal = P::new(-dir.y, dir.x);
    let dev: Vec<f64> = pts.iter().map(|&p| (p - origin).dot(normal)).collect();
    // Longer lines may stray a little more (Scale setting).
    let length = (pts[pts.len() - 1] - pts[0]).len();
    if dev.iter().any(|d| d.abs() > LINE_TOL + rel * length) {
        return None;
    }
    // Bow: least-squares parabola d(u) = c·(u² − mean u²) over u ∈ [−1, 1] along the line; an
    // arc shows up as a parabola, pixel ripples average out.
    let along: Vec<f64> = pts.iter().map(|&p| (p - origin).dot(dir)).collect();
    let (lo, hi) = (along[0], along[along.len() - 1]);
    let half = ((hi - lo) / 2.0).max(1e-9);
    let mid = f64::midpoint(lo, hi);
    let u: Vec<f64> = along.iter().map(|a| (a - mid) / half).collect();
    let mean_u2 = u.iter().map(|x| x * x).sum::<f64>() / u.len() as f64;
    let (mut num, mut den) = (0.0, 0.0);
    for (x, d) in u.iter().zip(&dev) {
        let b = x * x - mean_u2;
        num += b * d;
        den += b * b;
    }
    let bow = if den > 1e-12 { (num / den).abs() } else { 0.0 };
    (bow <= LINE_BOW + 0.5 * rel * length).then_some((origin, dir, dev))
}

/// Maximal straight parts of a span, in order, not overlapping.
fn find_lines(pts: &[P], rel: f64) -> Vec<Line> {
    let n = pts.len();
    let min_pts = 5;
    let mut out = Vec::new();
    let mut i = 0;
    while i + min_pts <= n {
        let mut j = i + min_pts - 1;
        if straight(&pts[i..=j], rel).is_none() {
            i += 1;
            continue;
        }
        while j + 1 < n && straight(&pts[i..=j + 1], rel).is_some() {
            j += 1;
        }
        // Trim the ends that already bend away (into the next curve).
        let (mut a, mut b) = (i, j);
        if let Some((_, _, dev)) = straight(&pts[a..=b], rel) {
            let mut lo = 0;
            let mut hi = dev.len() - 1;
            while hi > lo + min_pts && dev[lo].abs() > LINE_END_TOL {
                lo += 1;
            }
            while hi > lo + min_pts && dev[hi].abs() > LINE_END_TOL {
                hi -= 1;
            }
            (a, b) = (i + lo, i + hi);
        }
        match straight(&pts[a..=b], rel) {
            Some((origin, dir, _)) if (pts[b] - pts[a]).len() >= MIN_LINE_LEN && !on_arc(pts, a, b) => {
                let dir = snap_axis(dir, (pts[b] - pts[a]).len());
                out.push(Line { from: a, to: b, origin, dir });
                i = b + 1;
            }
            _ => i += 1,
        }
    }
    out
}

/// Whether the straight part `pts[a..=b]` is only a flat-looking piece of an arc: the part and
/// its neighbors on both sides (half its length each) lie on one circle.
fn on_arc(pts: &[P], a: usize, b: usize) -> bool {
    let k = (b - a) / 2 + 2;
    let (lo, hi) = (a.saturating_sub(k), (b + k).min(pts.len() - 1));
    if a - lo < 3 && hi - b < 3 {
        return false; // nothing around to compare with
    }
    let ext = &pts[lo..=hi];
    let Some((center, r)) = crate::shape::fit_circle(ext) else { return false };
    ext.iter().all(|&p| ((p - center).len() - r).abs() <= ARC_TOL)
}

/// Fit a span: its straight parts become lines, the turns between two of them circular arcs
/// tangent to both when they fit (fillets), the rest cubics tangent to the lines. `t1` is the
/// start tangent (direction of travel) and `t2` the end tangent pointing backward, when fixed.
/// The span's end points are kept exactly.
fn fit_span(
    span: &[P],
    t1: Option<P>,
    t2: Option<P>,
    params: FitParams,
    fillets: &mut Fillets,
    guides: &mut Guides,
    segs: &mut Vec<Seg>,
) {
    let n = span.len();
    let lines = if params.plain { Vec::new() } else { enabled_lines(span, params.relative, guides, true) };
    let mut starts: Vec<P> =
        lines.iter().map(|l| if l.from == 0 { span[0] } else { l.project(span[l.from]) }).collect();
    let mut ends: Vec<P> =
        lines.iter().map(|l| if l.to == n - 1 { span[n - 1] } else { l.project(span[l.to]) }).collect();
    // Arc after line i (to line i + 1).
    let mut arcs: Vec<Option<Vec<Seg>>> = vec![None; lines.len()];
    for i in 0..lines.len().saturating_sub(1) {
        let (a, b) = (&lines[i], &lines[i + 1]);
        let pts = &span[a.to..=b.from];
        let strict =
            fillet(pts, (a, starts[i]), (b, ends[i + 1]), params.arc_tol, params.relative, &fillets.snap);
        let within = strict.is_some();
        let found = strict.or_else(|| {
            fillet(pts, (a, starts[i]), (b, ends[i + 1]), CANDIDATE_ARC_TOL, params.relative, &[])
        });
        if let Some(f) = found
            && guides.add(GuideKind::Arc, f.middle, f.guide.clone(), within)
        {
            ends[i] = f.from;
            starts[i + 1] = f.to;
            arcs[i] = Some(f.segs);
            fillets.found.push(f.radius);
        }
    }
    let mut cursor = (0usize, span[0], t1);
    for (i, line) in lines.iter().enumerate() {
        let after_arc = i > 0 && arcs[i - 1].is_some();
        if line.from > 0 && !after_arc {
            curve_piece(span, cursor, (line.from, starts[i], Some(-line.dir)), params, guides, segs);
        }
        segs.push(Seg::Line(ends[i]));
        cursor = (line.to, ends[i], Some(line.dir));
        if let Some(arc) = &arcs[i] {
            segs.extend(arc.iter().copied());
        }
    }
    if cursor.0 < n - 1 {
        curve_piece(span, cursor, (n - 1, span[n - 1], t2), params, guides, segs);
    }
}

/// Straight parts of a span that are not turned off. `record`: add them to the guides (the
/// final fit of the span; a loop looks for lines once more only to choose its start).
fn enabled_lines(pts: &[P], rel: f64, guides: &mut Guides, record: bool) -> Vec<Line> {
    find_lines(pts, rel)
        .into_iter()
        .filter(|l| {
            let (a, b) = (l.project(pts[l.from]), l.project(pts[l.to]));
            let key = (a + b) * 0.5;
            if record {
                guides.add(GuideKind::Line, key, crate::guide::segment(a, b), true)
            } else {
                !guides.is_disabled(GuideKind::Line, key)
            }
        })
        .collect()
}

/// A circular arc tangent to two straight parts.
struct Fillet {
    /// Tangent points on the first and the second line.
    from: P,
    to: P,
    radius: f64,
    segs: Vec<Seg>,
    /// Middle of the arc (guide key) and path data drawing it.
    middle: P,
    guide: String,
}

/// Fit the arc tangent to `a` and `b` (each with its start / end point) that best follows
/// `pts` (the points from the end of `a` to the start of `b`), if every point stays within
/// `tol` of the line–arc–line path. A radius of `snap` is preferred when it fits as well.
fn fillet(pts: &[P], a: (&Line, P), b: (&Line, P), tol: f64, rel: f64, snap: &[f64]) -> Option<Fillet> {
    let ((la, a_start), (lb, b_end)) = (a, b);
    let (d1, d2) = (la.dir, lb.dir);
    let turn = acos(d1.dot(d2).clamp(-1.0, 1.0));
    if !(5f64.to_radians()..=175f64.to_radians()).contains(&turn) {
        return None;
    }
    let apex = intersect(la.origin, d1, lb.origin, d2)?;
    // Room for the tangent points: back along a to its start, forward along b to its end.
    let (room_a, room_b) = ((apex - a_start).dot(d1), (b_end - apex).dot(d2));
    if room_a <= 0.0 || room_b <= 0.0 {
        return None;
    }
    let half = (std::f64::consts::PI - turn) / 2.0; // half the interior angle
    let r_max = room_a.min(room_b) * tan(half);
    let bisector = (d2 - d1).normalized();
    let path = |r: f64| {
        let t = r / tan(half);
        let (t1, t2) = (apex - d1 * t, apex + d2 * t);
        let center = apex + bisector * (r / sin(half));
        (t1, t2, center)
    };
    // (max, rms) distance from the points to the line–arc–line path of radius r.
    let error = |r: f64| {
        let (t1, t2, center) = path(r);
        let (mut max, mut sum) = (0.0f64, 0.0);
        for &p in pts {
            let d = if (p - t1).dot(d1) <= 0.0 {
                cross(d1, p - t1).abs()
            } else if (p - t2).dot(d2) >= 0.0 {
                cross(d2, p - t2).abs()
            } else {
                ((p - center).len() - r).abs()
            };
            max = max.max(d);
            sum += d * d;
        }
        (max, (sum / pts.len() as f64).sqrt())
    };
    // Golden-section search of the radius on the RMS error.
    let (mut lo, mut hi) = (0.0, r_max);
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..40 {
        let (m1, m2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
        if error(m1).1 < error(m2).1 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let mut r = f64::midpoint(lo, hi);
    // Larger roundings may stray a little more (Scale setting).
    let tol = tol + rel * r;
    if r < MIN_FILLET || error(r).0 > tol {
        return None;
    }
    if let Some(&s) = snap
        .iter()
        .filter(|&&s| s <= r_max && (s - r).abs() <= SNAP_RADIUS.max(r * SNAP_RELATIVE))
        .min_by(|x, y| (*x - r).abs().total_cmp(&(*y - r).abs()))
        && error(s).0 <= tol
    {
        r = s;
    }
    let (t1, t2, center) = path(r);
    // Cubic quarter-or-less pieces: handle length 4/3·tan(φ/4)·r.
    let pieces = (turn / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    let phi = turn / pieces;
    let sense = cross(d1, d2).signum();
    let rot = |v: P, a: f64| {
        let (s, c) = sin_cos(a * sense);
        P::new(v.x * c - v.y * s, v.x * s + v.y * c)
    };
    let k = 4.0 / 3.0 * tan(phi / 4.0) * r;
    let mut segs = Vec::new();
    let mut radial = (t1 - center) * (1.0 / r);
    let mut dir = d1;
    for i in 0..pieces as usize {
        let next_radial = rot(radial, phi);
        let next_dir = rot(dir, phi);
        let p0 = center + radial * r;
        let p3 = if i + 1 == pieces as usize { t2 } else { center + next_radial * r };
        segs.push(Seg::Cubic(p0 + dir * k, p3 - next_dir * k, p3));
        (radial, dir) = (next_radial, next_dir);
    }
    let middle = center + ((t1 + t2) * 0.5 - center).normalized_or_x() * r;
    let guide = crate::guide::arc(t1, t2, r, sense > 0.0);
    Some(Fillet { from: t1, to: t2, radius: r, segs, middle, guide })
}

/// Smallest fillet radius (px): below, the turn is a corner, not an arc.
const MIN_FILLET: f64 = 0.75;

fn cross(a: P, b: P) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Cubics from break point `a` to `b` (index, position, tangent): `a`'s tangent points forward,
/// `b`'s backward; missing tangents are estimated from the points.
/// A piece that follows one circle becomes that circular arc (a guide); otherwise cubics.
fn curve_piece(
    span: &[P],
    a: (usize, P, Option<P>),
    b: (usize, P, Option<P>),
    params: FitParams,
    guides: &mut Guides,
    segs: &mut Vec<Seg>,
) {
    let mut pts = vec![a.1];
    pts.extend_from_slice(&span[a.0 + 1..b.0]);
    pts.push(b.1);
    if params.plain {
        let t1 = a.2.unwrap_or_else(|| end_tangent(pts.iter().copied()));
        let t2 = b.2.unwrap_or_else(|| end_tangent(pts.iter().rev().copied()));
        fit_cubic(&pts, t1, t2, params.tolerance, segs, 0);
        return;
    }
    let strict = free_arc(&pts, a.2, b.2, params.arc_tol, params.relative);
    let within = strict.is_some();
    let found = strict.or_else(|| free_arc(&pts, a.2, b.2, CANDIDATE_ARC_TOL, params.relative));
    if let Some(arc) = found
        && guides.add(GuideKind::Arc, arc.middle, arc.guide, within)
    {
        segs.extend(arc.segs);
        return;
    }
    let t1 = a.2.unwrap_or_else(|| end_tangent(pts.iter().copied()));
    let t2 = b.2.unwrap_or_else(|| end_tangent(pts.iter().rev().copied()));
    fit_cubic(&pts, t1, t2, params.tolerance, segs, 0);
}

/// A circular arc through the end points of a piece.
struct FreeArc {
    segs: Vec<Seg>,
    middle: P,
    guide: String,
}

/// Smallest turn (degrees) of a free arc: flatter pieces are left to the cubics.
const MIN_ARC_SWEEP: f64 = 25.0;
/// A fixed end tangent must match the arc's within this angle (degrees).
const ARC_TANGENT_SLACK: f64 = 4.0;

/// The circular arc from `pts[0]` to the last point that follows all the points within `tol`,
/// and respects the fixed end tangents (`t1` forward at the start, `t2` backward at the end).
fn free_arc(pts: &[P], t1: Option<P>, t2: Option<P>, tol: f64, rel: f64) -> Option<FreeArc> {
    let n = pts.len();
    if n < 6 || tol <= 0.0 {
        return None;
    }
    let (a, b) = (pts[0], pts[n - 1]);
    let chord = (b - a).len();
    if chord < 2.0 {
        return None;
    }
    // Centers on the perpendicular bisector of the chord: C(s) = mid + normal·s.
    let mid = (a + b) * 0.5;
    let normal = P::new(-(b - a).y, (b - a).x) * (1.0 / chord);
    let (kasa, _) = crate::shape::fit_circle(pts)?;
    let s0 = (kasa - mid).dot(normal);
    let error = |s: f64| {
        let c = mid + normal * s;
        let r = (a - c).len();
        let (mut max, mut sum) = (0.0f64, 0.0);
        for &p in pts {
            let d = ((p - c).len() - r).abs();
            max = max.max(d);
            sum += d * d;
        }
        (max, sum)
    };
    let (mut lo, mut hi) = (s0 - chord, s0 + chord);
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..40 {
        let (m1, m2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
        if error(m1).1 < error(m2).1 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let s = f64::midpoint(lo, hi);
    let center = mid + normal * s;
    let r = (a - center).len();
    // Larger arcs may stray a little more (Scale setting).
    if error(s).0 > tol + rel * r {
        return None;
    }
    // Signed sweep: the way from a to b that passes through the middle point.
    let angle = |p: P| atan2((p - center).y, (p - center).x);
    let tau = std::f64::consts::TAU;
    let (ta, tb, tm) = (angle(a), angle(b), angle(pts[n / 2]));
    let ccw = (tb - ta).rem_euclid(tau);
    let sweep = if (tm - ta).rem_euclid(tau) < ccw { ccw } else { ccw - tau };
    if sweep.abs() < MIN_ARC_SWEEP.to_radians() {
        return None;
    }
    let sense = sweep.signum();
    let tangent = |p: P| {
        let u = (p - center) * (1.0 / r);
        P::new(-u.y, u.x) * sense
    };
    let slack = cos(ARC_TANGENT_SLACK.to_radians());
    if t1.is_some_and(|t| t.dot(tangent(a)) < slack) || t2.is_some_and(|t| (-t).dot(tangent(b)) < slack) {
        return None;
    }
    // Cubic pieces of at most a quarter turn: handle 4/3·tan(φ/4)·r.
    let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
    let step = sweep / pieces;
    let k = 4.0 / 3.0 * tan(step.abs() / 4.0) * r;
    let at = |t: f64| center + P::new(cos(t), sin(t)) * r;
    let mut segs = Vec::new();
    for i in 0..pieces as usize {
        let (t0, t3) = (ta + step * i as f64, ta + step * (i + 1) as f64);
        let (p0, p3) = (at(t0), if i + 1 == pieces as usize { b } else { at(t3) });
        let (d0, d3) = (tangent(p0), tangent(at(t3)));
        let p0 = if i == 0 { a } else { p0 };
        segs.push(Seg::Cubic(p0 + d0 * k, p3 - d3 * k, p3));
    }
    let middle = at(ta + sweep / 2.0);
    let guide = if sweep.abs() < std::f64::consts::PI {
        crate::guide::arc(a, b, r, sense > 0.0)
    } else {
        // SVG arcs of 180° or more need the large-arc flag: draw two halves instead.
        crate::guide::arc(a, middle, r, sense > 0.0) + &crate::guide::arc(middle, b, r, sense > 0.0)
    };
    Some(FreeArc { segs, middle, guide })
}

/// Tangent points and arc (radius `r`, already clamped by the caller) of a rounded corner.
fn arc_anchors(run: &Prepared, c: &Corner, r: f64) -> [Anchor; 2] {
    let half = ((std::f64::consts::PI - c.turn) / 2.0).max(1e-3);
    let t = r / tan(half);
    let tin = c.apex - c.din * t;
    let tout = c.apex + c.dout * t;
    // Cubic approximation of the arc: handle length 4/3·tan(turn/4)·r.
    let h = 4.0 / 3.0 * tan(c.turn / 4.0) * r;
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

pub fn dist_to_segment(p: P, a: P, b: P) -> f64 {
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
        FitParams { tolerance: 0.7, arc_tol: 0.5, plain: false, relative: 0.0 }
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
        let run = prepare(v, &grid_mids(v, closed), ends, closed, None);
        fit_prepared(
            &run,
            &vec![CornerStyle::Sharp; run.corners.len()],
            params(),
            &mut Fillets::default(),
            &mut Guides::default(),
        )
    }

    fn square(n: i64) -> Vec<(i64, i64)> {
        let mut v = Vec::new();
        v.extend((0..n).map(|i| (i, 0)));
        v.extend((0..n).map(|i| (n, i)));
        v.extend((0..n).map(|i| (n - i, n)));
        v.extend((0..n).map(|i| (0, n - i)));
        v
    }

    /// Points every ~0.7 px along a closed outline given by `f(t)`, t ∈ [0, 1).
    fn sample(n: usize, f: impl Fn(f64) -> P) -> Prepared {
        let pts: Vec<P> = (0..n).map(|i| f(i as f64 / n as f64)).collect();
        prepare(&vec![(0, 0); n], &pts, (P::default(), P::default()), true, None)
    }

    fn lines(c: &Curve) -> usize {
        c.segs.iter().filter(|s| matches!(s, Seg::Line(_))).count()
    }

    #[test]
    fn straight_sides_become_lines_and_circles_do_not() {
        use std::f64::consts::TAU;
        // Rounded rectangle 120×60, corner radius 15: no corner, 4 straight sides.
        let (w, h, r) = (120.0, 60.0, 15.0);
        let (sw, sh, arc) = (w - 2.0 * r, h - 2.0 * r, TAU / 4.0 * r);
        let perimeter = 2.0 * (sw + sh) + 4.0 * arc;
        let rect = sample(400, |t| {
            let mut d = t * perimeter;
            let sides = [
                (sw, P::new(r, 0.0), 0.0),
                (sh, P::new(w, r), 1.0),
                (sw, P::new(w - r, h), 2.0),
                (sh, P::new(0.0, h - r), 3.0),
            ];
            for (len, start, q) in sides {
                let dir = P::new(cos(q * TAU / 4.0), sin(q * TAU / 4.0));
                if d < len {
                    return start + dir * d;
                }
                d -= len;
                if d < arc {
                    let center = start + dir * len + P::new(-dir.y, dir.x) * r;
                    let a = (q - 1.0) * TAU / 4.0 + d / r;
                    return center + P::new(cos(a), sin(a)) * r;
                }
                d -= arc;
            }
            P::new(r, 0.0)
        });
        // Candidates on the arcs stay rounded (free).
        let free = |p: &Prepared| vec![CornerStyle::Free; p.corners.len()];
        let mut fillets = Fillets::default();
        let c = fit_prepared(&rect, &free(&rect), params(), &mut fillets, &mut Guides::default());
        // 4 sides (the loop starts in the middle of one: 5 lines) and 4 tangent arcs of radius 15.
        assert_eq!(lines(&c), 5, "{c:?}");
        assert_eq!(fillets.found.len(), 4, "{c:?}");
        assert!(fillets.found.iter().all(|r| (r - 15.0).abs() < 0.3), "{:?}", fillets.found);

        // Its 4 roundings are corners whose apexes are the rectangle's corners; drawn sharp,
        // they give back the exact rectangle.
        let mut rect = rect;
        rect.add_fillet_corners(0.5, 0.0, &Guides::default());
        let mut apexes: Vec<(i64, i64)> = rect
            .corners
            .iter()
            .filter(|c| c.is_fillet())
            .map(|c| (c.apex.x.round() as i64, c.apex.y.round() as i64))
            .collect();
        apexes.sort_unstable();
        assert_eq!(apexes, [(0, 0), (0, 60), (120, 0), (120, 60)], "{:?}", rect.corners);
        let styles: Vec<CornerStyle> = rect
            .corners
            .iter()
            .map(|c| if c.is_fillet() { CornerStyle::Sharp } else { CornerStyle::Free })
            .collect();
        let c = fit_prepared(&rect, &styles, params(), &mut Fillets::default(), &mut Guides::default());
        assert!(c.segs.iter().all(|s| matches!(s, Seg::Line(_))), "{c:?}");
        for s in &c.segs {
            let e = s.end();
            assert!(
                (e.x.abs() < 0.3 || (e.x - 120.0).abs() < 0.3)
                    && (e.y.abs() < 0.3 || (e.y - 60.0).abs() < 0.3),
                "{c:?}"
            );
        }

        let circle = sample(300, |t| P::new(50.0 + 30.0 * cos(t * TAU), 50.0 + 30.0 * sin(t * TAU)));
        let c =
            fit_prepared(&circle, &free(&circle), params(), &mut Fillets::default(), &mut Guides::default());
        assert_eq!(lines(&c), 0, "{c:?}");
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
