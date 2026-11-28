//! Shapes of a synthetic scene, with their exact geometry (the ground truth).

use crate::num::math::{atan2, sin_cos, tan};
use std::f64::consts::PI;

/// A point in SVG coordinates (pixel (x, y) covers [x, x+1] × [y, y+1]).
pub type Pt = (f64, f64);

/// One closed cubic Bézier piece: start, two handles, end (the end is the next start).
pub type Cubic = [Pt; 4];

/// The exact outline of a shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Geom {
    /// Straight edges, sharp corners.
    Polygon(Vec<Pt>),
    /// A polygon whose vertex i is rounded by a circular arc of radius `radii[i]`, tangent to
    /// both edges (0 keeps the corner sharp). Radii too large for the edges are reduced.
    Rounded {
        vertices: Vec<Pt>,
        radii: Vec<f64>,
    },
    Circle {
        center: Pt,
        r: f64,
    },
    /// `angle` (radians) is the direction of the `a` axis.
    Ellipse {
        center: Pt,
        a: f64,
        b: f64,
        angle: f64,
    },
    /// A straight stroke of the given width with square-cut ends.
    Stroke {
        from: Pt,
        to: Pt,
        width: f64,
    },
    /// A closed path of cubic Bézier pieces.
    Bezier(Vec<Cubic>),
}

/// A feature the detectors should find, with its exact parameters.
#[derive(Debug, Clone, PartialEq)]
pub enum Feature {
    /// A corner at `vertex` (the intersection of the two edges), interior angle `angle`
    /// (radians, in (0, 2π)), rounded by `radius` (0 = sharp).
    Corner {
        vertex: Pt,
        angle: f64,
        radius: f64,
    },
    /// A straight part of an outline, between its two ends (or tangent points).
    Segment {
        from: Pt,
        to: Pt,
    },
    Circle {
        center: Pt,
        r: f64,
    },
    Ellipse {
        center: Pt,
        a: f64,
        b: f64,
        angle: f64,
    },
}

fn sub(a: Pt, b: Pt) -> Pt {
    (a.0 - b.0, a.1 - b.1)
}

fn add_scaled(a: Pt, d: Pt, k: f64) -> Pt {
    (a.0 + d.0 * k, a.1 + d.1 * k)
}

fn len(d: Pt) -> f64 {
    (d.0 * d.0 + d.1 * d.1).sqrt()
}

fn unit(d: Pt) -> Pt {
    let l = len(d);
    (d.0 / l, d.1 / l)
}

/// Number of chords so that an arc of radius `r` and sweep `sweep` is within `tol` of them.
fn arc_steps(r: f64, sweep: f64, tol: f64) -> u32 {
    let step = (8.0 * tol / r.max(tol)).sqrt();
    ((sweep.abs() / step).ceil() as u32).max(1)
}

/// Points of an arc around `c`, from angle `a0` turning by `sweep`, end point included.
fn arc_points(out: &mut Vec<Pt>, c: Pt, r: f64, a0: f64, sweep: f64, tol: f64) {
    let n = arc_steps(r, sweep, tol);
    for k in 1..=n {
        let (s, co) = sin_cos(a0 + sweep * f64::from(k) / f64::from(n));
        out.push((c.0 + r * co, c.1 + r * s));
    }
}

/// The fillet of vertex `v` between neighbours `prev` and `next`: tangent points, centre,
/// effective radius and interior angle of the two edges.
struct Fillet {
    t1: Pt,
    t2: Pt,
    center: Pt,
    radius: f64,
    angle: f64,
}

fn fillet(prev: Pt, v: Pt, next: Pt, radius: f64) -> Fillet {
    let (u1, u2) = (unit(sub(prev, v)), unit(sub(next, v)));
    let cos_phi = (u1.0 * u2.0 + u1.1 * u2.1).clamp(-1.0, 1.0);
    // Interior angle of the wedge between the two edges, in (0, π).
    let phi = atan2((u1.0 * u2.1 - u1.1 * u2.0).abs(), cos_phi);
    let room = 0.5 * len(sub(prev, v)).min(len(sub(next, v)));
    let half_tan = tan(0.5 * phi);
    let r = radius.min(room * half_tan);
    let t = r / half_tan;
    let bis = unit((u1.0 + u2.0, u1.1 + u2.1));
    let (sin_half, _) = sin_cos(0.5 * phi);
    Fillet {
        t1: add_scaled(v, u1, t),
        t2: add_scaled(v, u2, t),
        center: add_scaled(v, bis, r / sin_half),
        radius: r,
        angle: phi,
    }
}

fn signed_area(pts: &[Pt]) -> f64 {
    let mut a = 0.0;
    for (i, p) in pts.iter().enumerate() {
        let q = pts[(i + 1) % pts.len()];
        a += p.0 * q.1 - q.0 * p.1;
    }
    0.5 * a
}

impl Geom {
    /// Closed contours approximating the outline within `tol` px (holes turn the other way).
    pub fn contours(&self, tol: f64) -> Vec<Vec<Pt>> {
        match self {
            Geom::Polygon(p) => vec![p.clone()],
            Geom::Rounded { vertices, radii } => vec![rounded_outline(vertices, radii, tol)],
            Geom::Circle { center, r } => {
                let mut out = Vec::new();
                arc_points(&mut out, *center, *r, 0.0, 2.0 * PI, tol);
                vec![out]
            }
            Geom::Ellipse { center, a, b, angle } => {
                let n = arc_steps(a.max(*b), 2.0 * PI, tol);
                let (s, c) = sin_cos(*angle);
                let pts = (0..n)
                    .map(|k| {
                        let (st, ct) = sin_cos(2.0 * PI * f64::from(k) / f64::from(n));
                        let (x, y) = (a * ct, b * st);
                        (center.0 + c * x - s * y, center.1 + s * x + c * y)
                    })
                    .collect();
                vec![pts]
            }
            Geom::Stroke { from, to, width } => {
                let d = unit(sub(*to, *from));
                let n = (-d.1 * 0.5 * width, d.0 * 0.5 * width);
                vec![vec![
                    (from.0 + n.0, from.1 + n.1),
                    (to.0 + n.0, to.1 + n.1),
                    (to.0 - n.0, to.1 - n.1),
                    (from.0 - n.0, from.1 - n.1),
                ]]
            }
            Geom::Bezier(pieces) => {
                let mut out = Vec::new();
                for c in pieces {
                    flatten_cubic(&mut out, c, tol);
                }
                vec![out]
            }
        }
    }

    /// The features of the outline that a perfect vectorizer would recover.
    pub fn features(&self) -> Vec<Feature> {
        match self {
            Geom::Polygon(p) => polygon_features(p, &vec![0.0; p.len()]),
            Geom::Rounded { vertices, radii } => polygon_features(vertices, radii),
            Geom::Circle { center, r } => vec![Feature::Circle { center: *center, r: *r }],
            Geom::Ellipse { center, a, b, angle } => {
                vec![Feature::Ellipse { center: *center, a: *a, b: *b, angle: *angle }]
            }
            Geom::Stroke { .. } => {
                self.contours(1.0).iter().flat_map(|c| polygon_features(c, &[0.0; 4])).collect()
            }
            Geom::Bezier(_) => Vec::new(),
        }
    }
}

fn rounded_outline(vertices: &[Pt], radii: &[f64], tol: f64) -> Vec<Pt> {
    let n = vertices.len();
    let mut out = Vec::new();
    for i in 0..n {
        let (prev, v, next) = (vertices[(i + n - 1) % n], vertices[i], vertices[(i + 1) % n]);
        let r = radii.get(i).copied().unwrap_or(0.0);
        if r <= 0.0 {
            out.push(v);
            continue;
        }
        let f = fillet(prev, v, next, r);
        out.push(f.t1);
        let a0 = atan2(f.t1.1 - f.center.1, f.t1.0 - f.center.0);
        let a1 = atan2(f.t2.1 - f.center.1, f.t2.0 - f.center.0);
        let mut sweep = a1 - a0;
        while sweep > PI {
            sweep -= 2.0 * PI;
        }
        while sweep < -PI {
            sweep += 2.0 * PI;
        }
        arc_points(&mut out, f.center, f.radius, a0, sweep, tol);
    }
    out
}

fn polygon_features(vertices: &[Pt], radii: &[f64]) -> Vec<Feature> {
    let n = vertices.len();
    let ccw = signed_area(vertices) > 0.0;
    let mut out = Vec::new();
    let mut ends = Vec::with_capacity(n);
    for i in 0..n {
        let (prev, v, next) = (vertices[(i + n - 1) % n], vertices[i], vertices[(i + 1) % n]);
        let f = fillet(prev, v, next, radii.get(i).copied().unwrap_or(0.0));
        // Interior angle of the shape: the wedge angle at convex vertices, its complement to
        // 2π at reflex ones.
        let (e1, e2) = (sub(v, prev), sub(next, v));
        let turn_left = e1.0 * e2.1 - e1.1 * e2.0 > 0.0;
        let convex = turn_left == ccw;
        let angle = if convex { f.angle } else { 2.0 * PI - f.angle };
        out.push(Feature::Corner { vertex: v, angle, radius: f.radius });
        ends.push((f.t1, f.t2));
    }
    for i in 0..n {
        let from = ends[i].1;
        let to = ends[(i + 1) % n].0;
        out.push(Feature::Segment { from, to });
    }
    out
}

fn flatten_cubic(out: &mut Vec<Pt>, c: &Cubic, tol: f64) {
    // Wang's bound: n = ceil(sqrt(3/4 · max second difference / tol)).
    let dd = |a: Pt, b: Pt, d: Pt| len((a.0 - 2.0 * b.0 + d.0, a.1 - 2.0 * b.1 + d.1));
    let m = dd(c[0], c[1], c[2]).max(dd(c[1], c[2], c[3]));
    let n = ((0.75 * m / tol).sqrt().ceil() as u32).max(1);
    for k in 0..n {
        let t = f64::from(k) / f64::from(n);
        let u = 1.0 - t;
        let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
        out.push((
            b0 * c[0].0 + b1 * c[1].0 + b2 * c[2].0 + b3 * c[3].0,
            b0 * c[0].1 + b1 * c[1].1 + b2 * c[2].1 + b3 * c[3].1,
        ));
    }
}
