//! Whole shapes: a closed boundary that follows a circle or an ellipse is replaced by the exact
//! shape (four cubic quarter arcs).
//!
//! Ellipses use the direct least-squares fit of Fitzgibbon et al., in the numerically stable form
//! of Halíř & Flusser; circles the algebraic (Kåsa) fit. The circle wins when it fits about as
//! well (one parameter set less to get wrong).

use crate::fit::{Curve, P, Seg};

/// Largest distance (px) from a boundary point to the shape...
const MAX_ERR: f64 = 0.6;
/// ...and largest RMS distance.
const RMS_ERR: f64 = 0.22;
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
    fn distance(&self, p: P) -> f64 {
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
pub fn fit_round(pts: &[P]) -> Option<(Ellipse, bool)> {
    if pts.len() < 12 {
        return None;
    }
    let score = |e: &Ellipse| {
        let d: Vec<f64> = pts.iter().map(|&p| e.distance(p)).collect();
        let max = d.iter().copied().fold(0.0, f64::max);
        let rms = (d.iter().map(|x| x * x).sum::<f64>() / d.len() as f64).sqrt();
        (max, rms)
    };
    let ok = |e: &Ellipse, (max, rms): (f64, f64)| {
        e.b >= MIN_AXIS && e.a / e.b <= MAX_RATIO && max <= MAX_ERR && rms <= RMS_ERR
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
        let (fit, _) = fit_round(&ellipse_pts(&e, 300, 0.2)).expect("ellipse");
        assert!((fit.center - e.center).len() < 0.1, "{fit:?}");
        assert!((fit.a - e.a).abs() < 0.2 && (fit.b - e.b).abs() < 0.2, "{fit:?}");
        assert!((fit.angle - e.angle).sin().abs() < 0.01, "{fit:?}");
    }

    #[test]
    fn prefers_the_circle_and_rejects_squares() {
        let c = Ellipse { center: P::new(100.0, 100.0), a: 28.0, b: 28.0, angle: 0.0 };
        let (fit, _) = fit_round(&ellipse_pts(&c, 200, 0.15)).expect("circle");
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
        assert!(fit_round(&square).is_none());
    }

    #[test]
    fn curve_follows_the_boundary_direction() {
        let c = Ellipse { center: P::new(0.0, 0.0), a: 10.0, b: 5.0, angle: 0.3 };
        let pts = ellipse_pts(&c, 100, 0.0);
        let (_, reverse) = fit_round(&pts).expect("fit");
        assert!(!reverse);
        let rev: Vec<P> = pts.iter().rev().copied().collect();
        assert!(fit_round(&rev).expect("fit").1);
    }
}
