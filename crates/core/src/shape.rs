//! Whole shapes: a closed boundary that follows a circle or an ellipse is replaced by the exact
//! shape (four cubic quarter arcs).
//!
//! Ellipses use the direct least-squares fit of Fitzgibbon et al., in the numerically stable form
//! of Halíř & Flusser; circles the algebraic (Kåsa) fit. The circle wins when it fits about as
//! well (one parameter set less to get wrong).

use crate::fit::{Curve, P, Seg};

/// Largest distance (px) from a boundary point to the shape, and largest RMS distance, for the
/// geometry setting g: 0.6 and 0.22 at g = 0.5.
fn round_tolerance(g: f64) -> (f64, f64) {
    (0.3 + 0.6 * g, 0.1 + 0.24 * g)
}
/// Smallest semi-axis (px) and most elongated ratio considered.
const MIN_AXIS: f64 = 2.0;
const MAX_RATIO: f64 = 12.0;
/// A circle is preferred while its RMS error stays within this of the ellipse's.
const CIRCLE_BONUS: f64 = 0.04;
/// Cubic handle length of a quarter arc, relative to the radius.
const KAPPA: f64 = 0.552_284_749_830_793_6;

/// Center, semi-axes (a along `angle`, b across) and rotation of an ellipse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ellipse {
    pub center: P,
    pub a: f64,
    pub b: f64,
    pub angle: f64,
}

impl Ellipse {
    /// Approximate distance from `p` to the ellipse (accurate near it).
    pub fn distance(&self, p: P) -> f64 {
        let (s, c) = self.angle.sin_cos();
        let d = p - self.center;
        let (u, v) = (d.x * c + d.y * s, -d.x * s + d.y * c);
        // First-order (Sampson) distance: |f| / |∇f| with f = (u/a)² + (v/b)² − 1.
        let f = (u / self.a).powi(2) + (v / self.b).powi(2) - 1.0;
        let grad = (2.0 * u / (self.a * self.a)).hypot(2.0 * v / (self.b * self.b));
        if grad < 1e-12 { self.b } else { f.abs() / grad }
    }

    fn at(&self, t: f64) -> (P, P) {
        let (s, c) = self.angle.sin_cos();
        let (st, ct) = t.sin_cos();
        let rot = |x: f64, y: f64| P::new(x * c - y * s, x * s + y * c);
        (self.center + rot(self.a * ct, self.b * st), rot(-self.a * st, self.b * ct))
    }

    /// Parameter of the point of the ellipse nearest (radially) to `p`.
    fn param(&self, p: P) -> f64 {
        let (s, c) = self.angle.sin_cos();
        let d = p - self.center;
        let (u, v) = (d.x * c + d.y * s, -d.x * s + d.y * c);
        (v / self.b).atan2(u / self.a)
    }

    /// The part of the ellipse from `from` to `to` (points near it), going the way that passes
    /// by `via`, as cubics of at most a quarter turn. The curve starts and ends exactly at
    /// `from` and `to`.
    pub fn arc(&self, from: P, to: P, via: P) -> Curve {
        let tau = std::f64::consts::TAU;
        let (t0, t1, tv) = (self.param(from), self.param(to), self.param(via));
        let up = (t1 - t0).rem_euclid(tau);
        let sweep = if (tv - t0).rem_euclid(tau) < up { up } else { up - tau };
        let pieces = (sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.0);
        let step = sweep / pieces;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let count = pieces as usize;
        let segs = (0..count)
            .map(|i| {
                let (p0, d0) = self.at(t0 + step * i as f64);
                let (p3, d3) = self.at(t0 + step * (i + 1) as f64);
                // Pin the ends on the given points (they are within the fit tolerance).
                let (shift0, shift3) = (
                    if i == 0 { from - p0 } else { P::default() },
                    if i + 1 == count { to - p3 } else { P::default() },
                );
                Seg::Cubic(p0 + shift0 + d0 * k, p3 + shift3 - d3 * k, p3 + shift3)
            })
            .collect();
        Curve { start: from, segs }
    }

    /// The ellipse as four cubics, traversed in the direction of increasing parameter, or the
    /// reverse when `reverse`.
    pub fn curve(&self, reverse: bool) -> Curve {
        let quarter = std::f64::consts::FRAC_PI_2;
        let (start, _) = self.at(0.0);
        let segs = (0..4)
            .map(|k| {
                let (p0, d0) = self.at(f64::from(k) * quarter);
                let (p3, d3) = self.at(f64::from(k + 1) * quarter);
                Seg::Cubic(p0 + d0 * KAPPA, p3 - d3 * KAPPA, p3)
            })
            .collect();
        let curve = Curve { start, segs };
        if reverse { curve.reversed() } else { curve }
    }
}

/// The circle or ellipse that `pts` (a closed boundary) follows, if any, and whether the
/// boundary runs against its parametrization.
pub fn fit_round(pts: &[P], g: f64) -> Option<(Ellipse, bool)> {
    let (max_err, rms_err) = round_tolerance(g);
    if g <= 0.0 || pts.len() < 12 {
        return None;
    }
    let score = |e: &Ellipse| {
        let d: Vec<f64> = pts.iter().map(|&p| e.distance(p)).collect();
        let max = d.iter().copied().fold(0.0, f64::max);
        let rms = (d.iter().map(|x| x * x).sum::<f64>() / d.len() as f64).sqrt();
        (max, rms)
    };
    let ok = |e: &Ellipse, (max, rms): (f64, f64)| {
        e.b >= MIN_AXIS && e.a / e.b <= MAX_RATIO && max <= max_err && rms <= rms_err
    };
    let circle = fit_circle(pts).map(|(center, r)| Ellipse { center, a: r, b: r, angle: 0.0 });
    let ellipse = fit_ellipse(pts);
    let circle = circle.map(|e| (e, score(&e))).filter(|(e, s)| ok(e, *s));
    let ellipse = ellipse.map(|e| (e, score(&e))).filter(|(e, s)| ok(e, *s));
    let best = match (circle, ellipse) {
        (Some(c), Some(e)) => Some(if c.1.1 <= e.1.1 + CIRCLE_BONUS { c } else { e }),
        (c, e) => c.or(e),
    }?;
    // The parametrization turns from the a axis toward the b axis: positive shoelace area.
    let n = pts.len();
    let area: f64 = (0..n).map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y).sum();
    Some((best.0, area < 0.0))
}

/// Algebraic (Kåsa) circle fit: (center, radius).
pub fn fit_circle(pts: &[P]) -> Option<(P, f64)> {
    let k = 1.0 / pts.len() as f64;
    let c = pts.iter().fold(P::default(), |a, &p| a + p) * k;
    // Centered coordinates: minimize Σ (x² + y² + D x + E y + F)².
    let mut m = [[0.0; 3]; 3];
    let mut rhs = [0.0; 3];
    for &p in pts {
        let (x, y) = (p.x - c.x, p.y - c.y);
        let row = [x, y, 1.0];
        let z = x * x + y * y;
        for i in 0..3 {
            for j in 0..3 {
                m[i][j] += row[i] * row[j];
            }
            rhs[i] -= row[i] * z;
        }
    }
    let [d, e, f] = solve3(m, rhs)?;
    let r2 = (d * d + e * e) / 4.0 - f;
    (r2 > 0.0).then(|| (c + P::new(-d / 2.0, -e / 2.0), r2.sqrt()))
}

/// Direct least-squares ellipse fit (Fitzgibbon; Halíř & Flusser formulation).
pub fn fit_ellipse(pts: &[P]) -> Option<Ellipse> {
    // Normalize: centroid at the origin, mean distance 1.
    let k = 1.0 / pts.len() as f64;
    let c = pts.iter().fold(P::default(), |a, &p| a + p) * k;
    let s = pts.iter().map(|&p| (p - c).len()).sum::<f64>() * k;
    if s < 1e-9 {
        return None;
    }
    let mut s1 = [[0.0; 3]; 3];
    let mut s2 = [[0.0; 3]; 3];
    let mut s3 = [[0.0; 3]; 3];
    for &p in pts {
        let (x, y) = ((p.x - c.x) / s, (p.y - c.y) / s);
        let d1 = [x * x, x * y, y * y];
        let d2 = [x, y, 1.0];
        for i in 0..3 {
            for j in 0..3 {
                s1[i][j] += d1[i] * d1[j];
                s2[i][j] += d1[i] * d2[j];
                s3[i][j] += d2[i] * d2[j];
            }
        }
    }
    // T = −S3⁻¹ S2ᵀ (column by column), M = S1 + S2 T.
    let mut t = [[0.0; 3]; 3];
    for col in 0..3 {
        let rhs = [-s2[col][0], -s2[col][1], -s2[col][2]];
        let x = solve3(s3, rhs)?;
        for row in 0..3 {
            t[row][col] = x[row];
        }
    }
    let mut m = s1;
    for i in 0..3 {
        for j in 0..3 {
            m[i][j] += (0..3).map(|k| s2[i][k] * t[k][j]).sum::<f64>();
        }
    }
    // Premultiply by C1⁻¹ = [[0, 0, ½], [0, −1, 0], [½, 0, 0]].
    let m = [
        [m[2][0] / 2.0, m[2][1] / 2.0, m[2][2] / 2.0],
        [-m[1][0], -m[1][1], -m[1][2]],
        [m[0][0] / 2.0, m[0][1] / 2.0, m[0][2] / 2.0],
    ];
    let a1 = eigenvectors3(m).into_iter().find(|v| 4.0 * v[0] * v[2] - v[1] * v[1] > 0.0)?;
    let a2: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| t[i][j] * a1[j]).sum());
    let (qa, qb, qc, qd, qe, qf) = (a1[0], a1[1], a1[2], a2[0], a2[1], a2[2]);

    // Conic → center, axes, angle (normalized coordinates).
    let den = qb * qb - 4.0 * qa * qc;
    if den >= 0.0 {
        return None;
    }
    let x0 = (2.0 * qc * qd - qb * qe) / den;
    let y0 = (2.0 * qa * qe - qb * qd) / den;
    let f0 = qf + f64::midpoint(qd * x0, qe * y0);
    // Eigen-decomposition of [[A, B/2], [B/2, C]].
    let (h, half_b) = (f64::midpoint(qa, qc), qb / 2.0);
    let r = ((qa - qc) / 2.0).hypot(half_b);
    let (l1, l2) = (h - r, h + r);
    // Direction of the eigenvector of l1: (B/2, l1 − A), or an axis when B ≈ 0.
    let angle = if half_b.abs() > 1e-12 {
        (l1 - qa).atan2(half_b)
    } else if qa <= qc {
        0.0
    } else {
        std::f64::consts::FRAC_PI_2
    };
    let (ax2, bx2) = (-f0 / l1, -f0 / l2);
    if ax2 <= 0.0 || bx2 <= 0.0 {
        return None;
    }
    let (a, b) = (ax2.sqrt() * s, bx2.sqrt() * s);
    let center = c + P::new(x0, y0) * s;
    // Keep a as the major axis.
    Some(if a >= b {
        Ellipse { center, a, b, angle }
    } else {
        Ellipse { center, a: b, b: a, angle: angle + std::f64::consts::FRAC_PI_2 }
    })
}

/// Largest distance (px) between a boundary and its symmetric image, and largest RMS distance,
/// for the geometry setting g: 0.7 and 0.25 at g = 0.5.
fn symmetry_tolerance(g: f64) -> (f64, f64) {
    (0.35 + 0.7 * g, 0.1 + 0.3 * g)
}
/// Highest rotational order tried.
const MAX_ORDER: u32 = 12;
/// Samples used to search and test symmetries.
const SYM_SAMPLES: usize = 128;

/// Symmetries found on a closed boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Symmetry {
    /// Order of rotational symmetry (1 = none).
    pub order: u32,
    /// Has a mirror axis (then `order` axes in all).
    pub mirror: bool,
}

impl Symmetry {
    /// Indices of the images of point `i` of a symmetrized boundary of `n` points.
    pub fn images(self, i: usize, n: usize) -> impl Iterator<Item = usize> {
        let k = self.order.max(1) as usize;
        let step = n / k;
        let mirrored = if self.mirror { Some((n - i) % n) } else { None };
        (0..k).flat_map(move |j| {
            let rot = |p: usize| (p + j * step) % n;
            std::iter::once(rot(i)).chain(mirrored.map(rot))
        })
    }
}

/// A closed boundary as a function of arc length.
struct Loop {
    pts: Vec<P>,
    /// Cumulative length at each point (cum[0] = 0), total length last.
    cum: Vec<f64>,
}

impl Loop {
    fn new(pts: &[P]) -> Self {
        let n = pts.len();
        let mut cum = vec![0.0; n + 1];
        for i in 0..n {
            cum[i + 1] = cum[i] + (pts[(i + 1) % n] - pts[i]).len();
        }
        Loop { pts: pts.to_vec(), cum }
    }
    fn length(&self) -> f64 {
        self.cum[self.pts.len()]
    }
    /// Point at arc length `s` (any real; wraps around).
    fn at(&self, s: f64) -> P {
        let n = self.pts.len();
        let s = s.rem_euclid(self.length());
        let i = self.cum.partition_point(|&c| c <= s).clamp(1, n) - 1;
        let seg = self.cum[i + 1] - self.cum[i];
        let t = if seg > 1e-12 { (s - self.cum[i]) / seg } else { 0.0 };
        self.pts[i] + (self.pts[(i + 1) % n] - self.pts[i]) * t
    }
}

fn rotate(p: P, c: P, angle: f64) -> P {
    let (s, co) = angle.sin_cos();
    let d = p - c;
    c + P::new(d.x * co - d.y * s, d.x * s + d.y * co)
}

/// Mirror `p` about the line through `c` with unit direction `u`.
fn mirror(p: P, c: P, u: P) -> P {
    let d = p - c;
    let along = d.x * u.x + d.y * u.y;
    c + u * (2.0 * along) - d
}

/// (max, rms) distance between `f(s)` and `g(s)` over `samples` points of the loop.
fn compare(len: f64, samples: usize, f: impl Fn(f64) -> P, g: impl Fn(f64) -> P) -> (f64, f64) {
    let (mut max, mut sum) = (0.0f64, 0.0);
    for i in 0..samples {
        let s = len * i as f64 / samples as f64;
        let d = (f(s) - g(s)).len();
        max = max.max(d);
        sum += d * d;
    }
    (max, (sum / samples as f64).sqrt())
}

/// Make a closed boundary exactly symmetric when it nearly is: every point becomes the mean of
/// its images under the symmetries found (a mirror axis through the centroid, a rotation of
/// order k about it). Returns the new boundary: evenly spaced points, a multiple of k of them,
/// point 0 on the mirror axis, so that symmetries map point indices onto point indices (see
/// [`Symmetry::images`]).
pub fn symmetrize(pts: &[P], g: f64) -> Option<Symmetrized> {
    let n = pts.len();
    let (sym_max, sym_rms) = symmetry_tolerance(g);
    if g <= 0.0 || n < 16 {
        return None;
    }
    let lp = Loop::new(pts);
    let len = lp.length();
    // Area centroid; the sign of the area gives the turning sense of increasing s.
    let (mut a2, mut cx, mut cy) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let (p, q) = (pts[i], pts[(i + 1) % n]);
        let cross = p.x * q.y - q.x * p.y;
        a2 += cross;
        cx += (p.x + q.x) * cross;
        cy += (p.y + q.y) * cross;
    }
    if a2.abs() < 1e-9 {
        return None;
    }
    let c = P::new(cx / (3.0 * a2), cy / (3.0 * a2));
    let sense = a2.signum();
    let good = |(max, rms): (f64, f64)| max <= sym_max && rms <= sym_rms;

    // Rotation: the highest order whose rotated copy matches.
    let rot_err = |k: u32| {
        let shift = len / f64::from(k);
        let angle = -sense * std::f64::consts::TAU / f64::from(k);
        compare(len, SYM_SAMPLES, |s| lp.at(s), |s| rotate(lp.at(s + shift), c, angle))
    };
    let order = (2..=MAX_ORDER).rev().find(|&k| good(rot_err(k))).unwrap_or(1);

    // Mirror: the axis through the centroid and the boundary point at s0 maps s0 + t to s0 − t.
    let mir_err = |s0: f64| {
        let d = lp.at(s0) - c;
        let l = d.len();
        if l < 1e-9 {
            return (f64::MAX, f64::MAX);
        }
        let u = d * (1.0 / l);
        compare(len, SYM_SAMPLES, |t| lp.at(s0 + t), |t| mirror(lp.at(s0 - t), c, u))
    };
    // Coarse search over the boundary (half of it suffices: the axis crosses it twice), then a
    // golden-section refinement around the best sample.
    let step = len / SYM_SAMPLES as f64;
    let coarse = (0..=SYM_SAMPLES / 2)
        .map(|i| i as f64 * step)
        .min_by(|&a, &b| mir_err(a).1.total_cmp(&mir_err(b).1))
        .unwrap_or(0.0);
    let (mut lo, mut hi) = (coarse - step, coarse + step);
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..24 {
        let (m1, m2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
        if mir_err(m1).1 < mir_err(m2).1 {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    let s0 = f64::midpoint(lo, hi);
    let mirror_ok = good(mir_err(s0));
    if order == 1 && !mirror_ok {
        return None;
    }

    // Average over the symmetry group, sampled evenly from s0.
    let u = (lp.at(s0) - c).normalized_or_x();
    let p1 = |s: f64| {
        let p = lp.at(s0 + s);
        if mirror_ok { (p + mirror(lp.at(s0 - s), c, u)) * 0.5 } else { p }
    };
    // A multiple of the order: rotations then map sample indices onto sample indices.
    let n = n.div_ceil(order as usize) * order as usize;
    let out: Vec<P> = (0..n)
        .map(|i| {
            let s = len * i as f64 / n as f64;
            let mut sum = P::default();
            for j in 0..order {
                let shift = len * f64::from(j) / f64::from(order);
                let angle = -sense * std::f64::consts::TAU * f64::from(j) / f64::from(order);
                sum = sum + rotate(p1(s + shift), c, angle);
            }
            sum * (1.0 / f64::from(order))
        })
        .collect();
    // Guide: the mirror axes, or a small circle at the center of a rotation.
    let extent = pts.iter().map(|&p| (p - c).len()).fold(0.0, f64::max);
    // Beyond 6 axes (near-circles), one axis and the center are clearer than a sunburst.
    let drawn = if order > 6 { 1 } else { order };
    let center_mark = if order > 6 { crate::guide::ellipse(c, 2.0, 2.0, 0.0) } else { String::new() };
    let guide = if mirror_ok {
        center_mark
            + &(0..drawn)
                .map(|k| {
                    let v = rotate(u, P::default(), std::f64::consts::PI * f64::from(k) / f64::from(order));
                    crate::guide::segment(c - v * extent, c + v * extent)
                })
                .collect::<String>()
    } else {
        crate::guide::ellipse(c, 2.0, 2.0, 0.0)
    };
    let top = out.iter().copied().min_by(|p, q| p.y.total_cmp(&q.y)).unwrap_or(c);
    Some(Symmetrized { pts: out, kind: Symmetry { order, mirror: mirror_ok }, top, guide })
}

/// Result of [`symmetrize`].
pub struct Symmetrized {
    pub pts: Vec<P>,
    pub kind: Symmetry,
    /// Topmost point of the shape (guide key) and path data drawing the symmetries.
    pub top: P,
    pub guide: String,
}

impl Ellipse {
    /// Topmost point: the guide key (concentric shapes share a center, not a top).
    pub fn top(&self) -> P {
        let (s, c) = self.angle.sin_cos();
        let half_height = (self.a * self.a * s * s + self.b * self.b * c * c).sqrt();
        self.center - P::new(0.0, half_height)
    }

    /// Circle or ellipse guide.
    pub fn kind(&self) -> crate::guide::GuideKind {
        if (self.a - self.b).abs() < 1e-9 {
            crate::guide::GuideKind::Circle
        } else {
            crate::guide::GuideKind::Ellipse
        }
    }

    /// Path data drawing the ellipse.
    pub fn guide(&self) -> String {
        crate::guide::ellipse(self.center, self.a, self.b, self.angle)
    }
}

/// Solve m·x = rhs (Cramer's rule).
fn solve3(m: [[f64; 3]; 3], rhs: [f64; 3]) -> Option<[f64; 3]> {
    let det3 = |m: [[f64; 3]; 3]| {
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1]) - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    let det = det3(m);
    let scale = m.iter().flatten().fold(0.0f64, |a, v| a.max(v.abs()));
    if det.abs() <= 1e-12 * scale.powi(3) {
        return None;
    }
    Some(std::array::from_fn(|col| {
        let mut mm = m;
        for (row, v) in mm.iter_mut().zip(rhs) {
            row[col] = v;
        }
        det3(mm) / det
    }))
}

/// Eigenvectors of the real eigenvalues of a 3×3 matrix.
fn eigenvectors3(m: [[f64; 3]; 3]) -> Vec<[f64; 3]> {
    // Characteristic polynomial λ³ + a λ² + b λ + c.
    let tr = m[0][0] + m[1][1] + m[2][2];
    let minors = m[0][0] * m[1][1] - m[0][1] * m[1][0] + m[0][0] * m[2][2] - m[0][2] * m[2][0]
        + m[1][1] * m[2][2]
        - m[1][2] * m[2][1];
    let det = m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
        - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
        + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0]);
    let roots = cubic_roots(-tr, minors, -det);
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    };
    let norm2 = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>();
    roots
        .into_iter()
        .filter_map(|l| {
            let r: [[f64; 3]; 3] =
                std::array::from_fn(|i| std::array::from_fn(|j| m[i][j] - if i == j { l } else { 0.0 }));
            [cross(r[0], r[1]), cross(r[0], r[2]), cross(r[1], r[2])]
                .into_iter()
                .max_by(|a, b| norm2(*a).total_cmp(&norm2(*b)))
                .filter(|v| norm2(*v) > 1e-24)
        })
        .collect()
}

/// Real roots of x³ + a x² + b x + c.
fn cubic_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    let q = (a * a - 3.0 * b) / 9.0;
    let r = (2.0 * a * a * a - 9.0 * a * b + 27.0 * c) / 54.0;
    if r * r < q * q * q {
        let theta = (r / q.powf(1.5)).clamp(-1.0, 1.0).acos();
        let sq = -2.0 * q.sqrt();
        let tau = std::f64::consts::TAU;
        (0..3).map(|k| sq * ((theta + tau * f64::from(k)) / 3.0).cos() - a / 3.0).collect()
    } else {
        let big = -r.signum() * (r.abs() + (r * r - q * q * q).sqrt()).cbrt();
        let small = if big == 0.0 { 0.0 } else { q / big };
        vec![big + small - a / 3.0]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ellipse_pts(e: &Ellipse, n: usize, noise: f64) -> Vec<P> {
        (0..n)
            .map(|i| {
                let t = i as f64 / n as f64 * std::f64::consts::TAU;
                let (p, d) = e.at(t);
                // Alternate ±noise along the normal.
                let normal = P::new(d.y, -d.x) * (1.0 / d.len());
                p + normal * if i % 2 == 0 { noise } else { -noise }
            })
            .collect()
    }

    #[test]
    fn recovers_a_rotated_ellipse() {
        let e = Ellipse { center: P::new(210.0, 100.0), a: 50.0, b: 22.0, angle: -0.26 };
        let (fit, _) = fit_round(&ellipse_pts(&e, 300, 0.2), 0.5).expect("ellipse");
        assert!((fit.center - e.center).len() < 0.1, "{fit:?}");
        assert!((fit.a - e.a).abs() < 0.2 && (fit.b - e.b).abs() < 0.2, "{fit:?}");
        assert!((fit.angle - e.angle).sin().abs() < 0.01, "{fit:?}");
    }

    #[test]
    fn prefers_the_circle_and_rejects_squares() {
        let c = Ellipse { center: P::new(100.0, 100.0), a: 28.0, b: 28.0, angle: 0.0 };
        let (fit, _) = fit_round(&ellipse_pts(&c, 200, 0.15), 0.5).expect("circle");
        assert!((fit.a - fit.b).abs() < 1e-12 && (fit.a - 28.0).abs() < 0.1, "{fit:?}");

        let square: Vec<P> = (0..160)
            .map(|i| {
                let (side, t) = (i / 40, f64::from(i % 40));
                match side {
                    0 => P::new(t, 0.0),
                    1 => P::new(40.0, t),
                    2 => P::new(40.0 - t, 40.0),
                    _ => P::new(0.0, 40.0 - t),
                }
            })
            .collect();
        assert!(fit_round(&square, 0.5).is_none());
    }

    /// A 5-pointed star (outer radius 40, inner 16) with alternating noise, rotated by `phase`.
    fn star(noise: f64, phase: f64) -> Vec<P> {
        let tips: Vec<P> = (0..10)
            .map(|i| {
                let r = if i % 2 == 0 { 40.0 } else { 16.0 };
                let a = phase + f64::from(i) * std::f64::consts::PI / 5.0;
                P::new(100.0 + r * a.cos(), 100.0 + r * a.sin())
            })
            .collect();
        let mut pts = Vec::new();
        for i in 0..10 {
            let (a, b) = (tips[i], tips[(i + 1) % 10]);
            for k in 0..30 {
                let p = a + (b - a) * (f64::from(k) / 30.0);
                pts.push(p + P::new(0.0, if pts.len() % 3 == 0 { noise } else { -noise / 2.0 }));
            }
        }
        pts
    }

    #[test]
    fn star_becomes_exactly_symmetric() {
        let found = symmetrize(&star(0.1, 0.3), 0.5).expect("symmetric");
        let (sym, kind) = (found.pts, found.kind);
        assert_eq!(kind, Symmetry { order: 5, mirror: true });
        // Every tip at the same distance from the center, within 0.05 px.
        let c = P::new(100.0, 100.0);
        let far: Vec<f64> = (0..5)
            .map(|k| {
                let a = 0.3 + f64::from(k) * std::f64::consts::TAU / 5.0;
                let dir = P::new(a.cos(), a.sin());
                sym.iter().map(|&p| (p - c).dot(dir)).fold(f64::MIN, f64::max)
            })
            .collect();
        let (lo, hi) = far.iter().fold((f64::MAX, f64::MIN), |(l, h), &v| (l.min(v), h.max(v)));
        assert!(hi - lo < 0.05, "{far:?}");
    }

    #[test]
    fn asymmetric_shape_is_left_alone() {
        // A right triangle with legs 60 and 25 has no symmetry.
        let corners = [P::new(0.0, 0.0), P::new(60.0, 0.0), P::new(0.0, 25.0)];
        let mut pts = Vec::new();
        for i in 0..3 {
            let (a, b) = (corners[i], corners[(i + 1) % 3]);
            for k in 0..40 {
                pts.push(a + (b - a) * (f64::from(k) / 40.0));
            }
        }
        assert!(symmetrize(&pts, 0.5).is_none());
    }

    #[test]
    fn curve_follows_the_boundary_direction() {
        let c = Ellipse { center: P::new(0.0, 0.0), a: 10.0, b: 5.0, angle: 0.3 };
        let pts = ellipse_pts(&c, 100, 0.0);
        let (_, reverse) = fit_round(&pts, 0.5).expect("fit");
        assert!(!reverse);
        let rev: Vec<P> = pts.iter().rev().copied().collect();
        assert!(fit_round(&rev, 0.5).expect("fit").1);
    }
}
