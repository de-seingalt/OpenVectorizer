//! Repeated shapes: isolated shapes that are copies of one another (the stars of a flag, dots,
//! repeated letters) get exactly the same outline. Each group is replaced by its mean shape,
//! moved and rotated onto every member.

use std::collections::HashMap;

use crate::fit::P;
use crate::guide::{CANDIDATE_GEOMETRY, GuideKind, Guides};
use crate::shape::{Loop, RELATIVE_MAX, RELATIVE_RMS};

/// Samples used to compare outlines.
const SAMPLES: usize = 64;
/// Shapes whose area and perimeter differ by more than this fraction are not compared.
const SIZE_SLACK: f64 = 0.05;

/// Largest distance (px) and RMS distance between a shape and a moved copy of another, for
/// the geometry setting g: 0.8 and 0.3 at g = 0.5.
fn tolerance(g: f64) -> (f64, f64) {
    (0.35 + 0.9 * g, 0.1 + 0.4 * g)
}

/// An outline turning in the positive sense, and what is needed to compare it.
struct Shape {
    lp: Loop,
    /// The input turned the other way (it was reversed).
    flipped: bool,
    area: f64,
    center: P,
    n: usize,
    /// Distance to the center at `SAMPLES` evenly spaced points, and the same sorted.
    radial: Vec<f64>,
    sorted: Vec<f64>,
}

impl Shape {
    fn new(pts: &[P]) -> Option<Shape> {
        let n = pts.len();
        if n < 12 {
            return None;
        }
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
        let center = P::new(cx / (3.0 * a2), cy / (3.0 * a2));
        let flipped = a2 < 0.0;
        let oriented: Vec<P> = if flipped { pts.iter().rev().copied().collect() } else { pts.to_vec() };
        let lp = Loop::new(&oriented);
        let radial =
            (0..SAMPLES).map(|i| (lp.at(lp.length() * i as f64 / SAMPLES as f64) - center).len()).collect();
        let radial: Vec<f64> = radial;
        let mut sorted = radial.clone();
        sorted.sort_by(f64::total_cmp);
        Some(Shape { lp, flipped, area: a2.abs() / 2.0, center, n, radial, sorted })
    }

    fn sample(&self, offset: f64, i: usize, count: usize) -> P {
        self.lp.at(offset + self.lp.length() * i as f64 / count as f64)
    }

    fn close_in_size(&self, o: &Shape) -> bool {
        let near = |a: f64, b: f64| (a - b).abs() <= SIZE_SLACK * a.max(b);
        near(self.area, o.area) && near(self.lp.length(), o.lp.length())
    }
}

/// How `b` is a copy of `a`: the arc length on `b` where `a`'s start lands, the rotation, and
/// the (max, rms) distance between `b` and the moved copy of `a`.
struct Match {
    offset: f64,
    angle: f64,
    max: f64,
    rms: f64,
}

fn rotate(v: P, angle: f64) -> P {
    let (s, c) = angle.sin_cos();
    P::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// Largest difference between the sorted radial profiles: a lower bound of the radial
/// difference under any correspondence, so a very cheap first test.
fn sorted_gap(a: &Shape, b: &Shape) -> f64 {
    a.sorted.iter().zip(&b.sorted).map(|(x, y)| (x - y).abs()).fold(0.0, f64::max)
}

/// Radial profile comparison at every sample shift: the shift that matches best, and whether
/// some shift keeps the largest and the RMS radial differences within `tol` (plus `slack`). A
/// rotation keeps distances to the center, so those differences are (up to the sampling step)
/// lower bounds of the outline distances: a cheap way to rule pairs out.
fn radial_match(a: &Shape, b: &Shape, tol: (f64, f64), slack: f64) -> (usize, bool) {
    let mut best = (0, f64::MAX);
    let mut possible = false;
    for k in 0..SAMPLES {
        let (mut sum, mut max) = (0.0f64, 0.0f64);
        for i in 0..SAMPLES {
            let d = (a.radial[i] - b.radial[(i + k) % SAMPLES]).abs();
            sum += d * d;
            max = max.max(d);
        }
        if sum < best.1 {
            best = (k, sum);
        }
        possible |= max <= tol.0 + slack && (sum / SAMPLES as f64).sqrt() <= tol.1 + slack;
    }
    (best.0, possible)
}

/// Best rigid match (rotation about the centers, then translation) of `a` onto `b`.
fn align(a: &Shape, b: &Shape) -> Match {
    let pa: Vec<P> = (0..SAMPLES).map(|i| a.sample(0.0, i, SAMPLES) - a.center).collect();
    let fit = |offset: f64| {
        let pb: Vec<P> = (0..SAMPLES).map(|i| b.sample(offset, i, SAMPLES) - b.center).collect();
        // Procrustes rotation: atan2(Σ a×b, Σ a·b).
        let (mut cross, mut dot) = (0.0, 0.0);
        for (u, v) in pa.iter().zip(&pb) {
            cross += u.x * v.y - u.y * v.x;
            dot += u.dot(*v);
        }
        let angle = cross.atan2(dot);
        let (mut max, mut sum) = (0.0f64, 0.0);
        for (u, v) in pa.iter().zip(&pb) {
            let d = (rotate(*u, angle) - *v).len();
            max = max.max(d);
            sum += d * d;
        }
        Match { offset, angle, max, rms: (sum / SAMPLES as f64).sqrt() }
    };
    // Coarse: radial signatures at every sample shift; then refine the offset continuously.
    let step = b.lp.length() / SAMPLES as f64;
    let shift = radial_match(a, b, (f64::MAX, f64::MAX), 0.0).0;
    let (mut lo, mut hi) = ((shift as f64 - 1.0) * step, (shift as f64 + 1.0) * step);
    let phi = (5f64.sqrt() - 1.0) / 2.0;
    for _ in 0..20 {
        let (m1, m2) = (hi - phi * (hi - lo), lo + phi * (hi - lo));
        if fit(m1).rms < fit(m2).rms {
            hi = m2;
        } else {
            lo = m1;
        }
    }
    fit(f64::midpoint(lo, hi))
}

/// New outlines for the closed outlines `loops` that repeat another one (`None`: unchanged),
/// point for point (same number of points, each moved onto the shared outline).
/// Every member of a group but its first (the reference) is a guide that can be turned off
/// or, when it matches only loosely, forced.
pub fn unify(loops: &[Vec<P>], g: f64, rel: f64, guides: &mut Guides) -> Vec<Option<Vec<P>>> {
    let shapes: Vec<Option<Shape>> = loops.iter().map(|l| Shape::new(l)).collect();
    let (strict, loose) = (tolerance(g), tolerance(CANDIDATE_GEOMETRY));
    // Groups: reference index, then (member, match). References are filed by size (area and
    // perimeter on a 5 % log scale) so that a shape is only compared with those of its size.
    let mut groups: Vec<(usize, Vec<(usize, Match)>)> = Vec::new();
    let mut by_size: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let bucket = |s: &Shape| {
        let q = |v: f64| (v.max(1e-9).ln() / (1.0 + SIZE_SLACK).ln()).floor() as i64;
        (q(s.area), q(s.lp.length()))
    };
    for (i, shape) in shapes.iter().enumerate() {
        let Some(shape) = shape else { continue };
        let (ba, bp) = bucket(shape);
        let mut candidates: Vec<usize> = (ba - 1..=ba + 1)
            .flat_map(|a| (bp - 1..=bp + 1).map(move |p| (a, p)))
            .filter_map(|key| by_size.get(&key))
            .flatten()
            .copied()
            .collect();
        candidates.sort_unstable();
        let slack = shape.lp.length() / SAMPLES as f64;
        // Small shapes get a tolerance in proportion to their size.
        // Larger shapes may stray a little more (Scale setting).
        let extra = rel * shape.lp.length() / std::f64::consts::TAU;
        let cap = |(max, rms): (f64, f64)| {
            (
                max.min(RELATIVE_MAX * shape.lp.length()) + extra,
                rms.min(RELATIVE_RMS * shape.lp.length()) + 0.4 * extra,
            )
        };
        let (strict, loose) = (cap(strict), cap(loose));
        let mut placed = false;
        for g_idx in candidates {
            let (lead, members) = &mut groups[g_idx];
            let Some(reference) = &shapes[*lead] else { continue };
            if !reference.close_in_size(shape)
                || sorted_gap(reference, shape) > loose.0 + slack
                || !radial_match(reference, shape, loose, slack).1
            {
                continue;
            }
            let m = align(reference, shape);
            if m.max > loose.0 || m.rms > loose.1 {
                continue;
            }
            placed = true;
            let within = g > 0.0 && m.max <= strict.0 && m.rms <= strict.1;
            let top = loops[i].iter().copied().min_by(|p, q| p.y.total_cmp(&q.y)).unwrap_or(shape.center);
            let outline = moved_outline(reference, shape, &m);
            if guides.add(GuideKind::Repeat, top, outline, within) {
                members.push((i, m));
            }
            break;
        }
        if !placed {
            by_size.entry((ba, bp)).or_default().push(groups.len());
            groups.push((i, Vec::new()));
        }
    }

    let mut out: Vec<Option<Vec<P>>> = vec![None; loops.len()];
    for (lead, members) in groups.iter().filter(|(_, m)| !m.is_empty()) {
        let Some(reference) = &shapes[*lead] else { continue };
        let count = members
            .iter()
            .filter_map(|(i, _)| shapes[*i].as_ref().map(|s| s.n))
            .fold(reference.n, usize::max);
        // Mean outline in the reference's frame.
        let mut mean: Vec<P> = (0..count).map(|i| reference.sample(0.0, i, count)).collect();
        for (i, m) in members {
            let Some(s) = &shapes[*i] else { continue };
            for (k, p) in mean.iter_mut().enumerate() {
                let back = reference.center + rotate(s.sample(m.offset, k, count) - s.center, -m.angle);
                *p = *p + back;
            }
        }
        let weight = 1.0 / (members.len() + 1) as f64;
        for p in &mut mean {
            *p = *p * weight;
        }
        // The mean outline moved onto a shape, evaluated at each of its original points (so the
        // result lines up with the input, point for point).
        let place = |i: usize, s: &Shape, offset: f64, angle: f64| {
            let orig = &loops[i];
            let n = orig.len();
            let mut cum = vec![0.0; n];
            for j in 1..n {
                cum[j] = cum[j - 1] + (orig[j] - orig[j - 1]).len();
            }
            let len = s.lp.length();
            (0..n)
                .map(|j| {
                    // Arc length of point j along the outline as oriented for the comparison.
                    let along = if s.flipped { cum[n - 1] - cum[j] } else { cum[j] };
                    let u = (along - offset).rem_euclid(len) / len * count as f64;
                    let (k, t) = (u.floor() as usize % count, u.fract());
                    let p = mean[k] * (1.0 - t) + mean[(k + 1) % count] * t;
                    s.center + rotate(p - reference.center, angle)
                })
                .collect::<Vec<P>>()
        };
        out[*lead] = Some(place(*lead, reference, 0.0, 0.0));
        for (i, m) in members {
            if let Some(s) = &shapes[*i] {
                out[*i] = Some(place(*i, s, m.offset, m.angle));
            }
        }
    }
    out
}

/// Path data of `reference` moved onto `shape` (a closed polyline).
fn moved_outline(reference: &Shape, shape: &Shape, m: &Match) -> String {
    let pts: Vec<P> = (0..SAMPLES)
        .map(|i| shape.center + rotate(reference.sample(0.0, i, SAMPLES) - reference.center, m.angle))
        .collect();
    crate::guide::polygon(&pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5-pointed star outline, rotated by `phase`, at `center`, with a small `wobble`.
    fn star(center: P, phase: f64, wobble: f64) -> Vec<P> {
        let tips: Vec<P> = (0..10)
            .map(|i| {
                let r = if i % 2 == 0 { 20.0 } else { 8.0 };
                let a = phase + f64::from(i) * std::f64::consts::PI / 5.0;
                center + P::new(r * a.cos(), r * a.sin())
            })
            .collect();
        let mut pts = Vec::new();
        for i in 0..10 {
            let (a, b) = (tips[i], tips[(i + 1) % 10]);
            for k in 0..12 {
                let w = if pts.len() % 2 == 0 { wobble } else { -wobble };
                pts.push(a + (b - a) * (f64::from(k) / 12.0) + P::new(0.0, w));
            }
        }
        pts
    }

    #[test]
    fn copies_get_the_same_outline() {
        let loops = vec![
            star(P::new(30.0, 30.0), 0.0, 0.15),
            star(P::new(100.0, 40.0), 0.4, -0.15),
            // A different shape: a bigger star.
            star(P::new(60.0, 100.0), 0.0, 0.0)
                .iter()
                .map(|&p| P::new(60.0, 100.0) + (p - P::new(60.0, 100.0)) * 1.5)
                .collect(),
        ];
        let mut guides = Guides::default();
        let out = unify(&loops, 0.5, 0.0, &mut guides);
        assert!(out[2].is_none());
        let (a, b) = (out[0].as_ref().expect("reference"), out[1].as_ref().expect("copy"));
        // Point for point with the inputs...
        assert_eq!((a.len(), b.len()), (loops[0].len(), loops[1].len()));
        // ...and the copy is the reference turned by 0.4 rad and moved: every point of the copy
        // lies on the moved reference outline.
        let (ca, cb) = (P::new(30.0, 30.0), P::new(100.0, 40.0));
        let moved: Vec<P> = a.iter().map(|&p| cb + rotate(p - ca, 0.4)).collect();
        let dist = |q: P| {
            (0..moved.len())
                .map(|i| crate::fit::dist_to_segment(q, moved[i], moved[(i + 1) % moved.len()]))
                .fold(f64::MAX, f64::min)
        };
        for &q in b {
            assert!(dist(q) < 0.25, "{q:?} is {} px off", dist(q));
        }
        assert_eq!(guides.found.len(), 1);
    }
}
