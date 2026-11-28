//! Global alignments: straight segments of the whole image that lie on one line, even far apart
//! or interrupted, are put exactly on that line (the long lines of a drawing). A vertex where two
//! such lines meet goes to their intersection. Vertices are moved by position, so the ends shared
//! by several runs move together and the drawing stays watertight.

use std::collections::BTreeMap;

use crate::fit::{Curve, P, Seg, fit_line, intersect, snap_axis};
use crate::guide::{GuideKind, Guides};

/// Segments shorter than this (px) do not make or join alignments.
const MIN_SEGMENT: f64 = 6.0;

/// Segments overlapping along a line and this far apart across it (px) are side by side.
const SIDE_BY_SIDE: f64 = 0.25;

/// A straight segment of a run's curve.
struct Segment {
    a: P,
    b: P,
}

/// A line shared by several segments.
struct Alignment {
    members: Vec<usize>,
    origin: P,
    dir: P,
}

impl Alignment {
    fn distance(&self, p: P) -> f64 {
        let d = p - self.origin;
        (d.x * self.dir.y - d.y * self.dir.x).abs()
    }
    fn project(&self, p: P) -> P {
        self.origin + self.dir * (p - self.origin).dot(self.dir)
    }
    /// Whether segment `a`–`b` runs alongside a member instead of continuing the line: their
    /// stretches along the line overlap while they are apart across it (the two edges of a
    /// thin stroke are two lines, not one).
    fn side_by_side(&self, segments: &[Segment], a: P, b: P) -> bool {
        let along = |p: P| (p - self.origin).dot(self.dir);
        let across = |p: P| {
            let d = p - self.origin;
            d.x * self.dir.y - d.y * self.dir.x
        };
        let (lo, hi) = (along(a).min(along(b)), along(a).max(along(b)));
        let offset = f64::midpoint(across(a), across(b));
        self.members.iter().any(|&m| {
            let (c, d) = (segments[m].a, segments[m].b);
            let overlap = hi.min(along(c).max(along(d))) - lo.max(along(c).min(along(d)));
            overlap > 1.0 && (f64::midpoint(across(c), across(d)) - offset).abs() > SIDE_BY_SIDE
        })
    }

    /// Refit on the end points of the members (weighted by their length).
    fn refit(&mut self, segments: &[Segment]) {
        let mut pts = Vec::new();
        let mut extent = 0.0f64;
        for &m in &self.members {
            let (a, b) = (segments[m].a, segments[m].b);
            let k = ((b - a).len() / 2.0).ceil().max(1.0) as usize;
            pts.extend((0..=k).map(|i| a + (b - a) * (i as f64 / k as f64)));
            extent = extent.max((b - a).len());
        }
        if let Some((origin, dir)) = fit_line(&pts) {
            let span = pts
                .iter()
                .map(|&p| (p - origin).dot(dir))
                .fold((f64::MAX, f64::MIN), |(l, h), t| (l.min(t), h.max(t)));
            self.origin = origin;
            self.dir = snap_axis(dir, span.1 - span.0);
        }
    }
}

/// Put the straight segments of `curves` that share a line onto it. `tol` (and `loose` for
/// candidates) is how far a segment's ends may be from the line, plus `rel` × its length.
pub fn align_lines(curves: &mut [Curve], (tol, loose, rel): (f64, f64, f64), guides: &mut Guides) {
    let mut segments = Vec::new();
    for curve in curves.iter() {
        let mut from = curve.start;
        for seg in &curve.segs {
            if let Seg::Line(e) = *seg
                && (e - from).len() >= MIN_SEGMENT
            {
                segments.push(Segment { a: from, b: e });
            }
            from = seg.end();
        }
    }
    // Longest first: long segments define the lines, short ones join them.
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by(|&x, &y| {
        (segments[y].b - segments[y].a).len().total_cmp(&(segments[x].b - segments[x].a).len())
    });
    let mut lines: Vec<Alignment> = Vec::new();
    for &s in &order {
        let (a, b) = (segments[s].a, segments[s].b);
        let reach = loose + rel * (b - a).len();
        let found = lines
            .iter_mut()
            .find(|l| l.distance(a) <= reach && l.distance(b) <= reach && !l.side_by_side(&segments, a, b));
        match found {
            Some(line) => {
                line.members.push(s);
                line.refit(&segments);
            }
            None => {
                lines.push(Alignment { members: vec![s], origin: a, dir: (b - a) * (1.0 / (b - a).len()) });
            }
        }
    }

    // Constraints per vertex position: the lines it must lie on.
    let key = |p: P| (p.x.to_bits(), p.y.to_bits());
    let mut on: BTreeMap<(u64, u64), Vec<usize>> = BTreeMap::new();
    for (l, line) in lines.iter().enumerate().filter(|(_, l)| l.members.len() >= 2) {
        let ends: Vec<P> = line.members.iter().flat_map(|&m| [segments[m].a, segments[m].b]).collect();
        let worst = line
            .members
            .iter()
            .map(|&m| {
                let (a, b) = (segments[m].a, segments[m].b);
                line.distance(a).max(line.distance(b)) - rel * (b - a).len()
            })
            .fold(0.0, f64::max);
        let span = ends
            .iter()
            .map(|&p| (p - line.origin).dot(line.dir))
            .fold((f64::MAX, f64::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let (p0, p1) = (line.origin + line.dir * span.0, line.origin + line.dir * span.1);
        let key_point = (p0 + p1) * 0.5;
        if !guides.add(GuideKind::Align, key_point, crate::guide::segment(p0, p1), worst <= tol) {
            continue;
        }
        for p in ends {
            let list = on.entry(key(p)).or_default();
            if !list.contains(&l) {
                list.push(l);
            }
        }
    }
    if on.is_empty() {
        return;
    }
    // Where each constrained vertex goes: onto its line, or the intersection of two.
    let moves: BTreeMap<(u64, u64), P> = on
        .iter()
        .filter_map(|(&k, ls)| {
            let p = P::new(f64::from_bits(k.0), f64::from_bits(k.1));
            let to = match ls[..] {
                [l] => lines[l].project(p),
                [l, m, ..] => {
                    let (a, b) = (&lines[l], &lines[m]);
                    intersect(a.origin, a.dir, b.origin, b.dir)
                        .filter(|q| (*q - p).len() <= 2.0 * loose)
                        .unwrap_or_else(|| a.project(p))
                }
                [] => return None,
            };
            Some((k, to))
        })
        .collect();
    // Move the vertices; the neighbouring control points follow (tangents keep their direction).
    for curve in curves.iter_mut() {
        let shift = |p: P| moves.get(&key(p)).map_or(P::default(), |&q| q - p);
        let start_shift = shift(curve.start);
        curve.start = curve.start + start_shift;
        let mut prev_shift = start_shift;
        for seg in &mut curve.segs {
            let end = seg.end();
            let s = shift(end);
            *seg = match *seg {
                Seg::Line(e) => Seg::Line(e + s),
                Seg::Cubic(c1, c2, e) => Seg::Cubic(c1 + prev_shift, c2 + s, e + s),
            };
            prev_shift = s;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: (f64, f64), b: (f64, f64)) -> Curve {
        Curve { start: P::new(a.0, a.1), segs: vec![Seg::Line(P::new(b.0, b.1))] }
    }

    #[test]
    fn interrupted_line_is_aligned_and_strokes_are_not_merged() {
        let mut curves = vec![
            // One line interrupted by a gap, its pieces a little off.
            line((0.0, 10.2), (20.0, 9.9)),
            line((40.0, 10.1), (60.0, 9.8)),
            // A parallel line 5 px away: a different line.
            line((0.0, 15.0), (60.0, 15.2)),
            // The two edges of a 1.5 px stroke: side by side, not one line.
            line((0.0, 30.0), (60.0, 30.0)),
            line((0.0, 31.5), (60.0, 31.5)),
        ];
        let mut guides = Guides::default();
        align_lines(&mut curves, (0.5, 1.2, 0.0), &mut guides);
        let ends = |c: &Curve| (c.start, c.segs[0].end());
        let ((a, b), (c, d)) = (ends(&curves[0]), ends(&curves[1]));
        // All four ends on one line (here exactly horizontal after the axis snap).
        for p in [a, b, c, d] {
            assert!((p.y - a.y).abs() < 1e-9, "{a:?} {b:?} {c:?} {d:?}");
        }
        assert!((a.y - 10.0).abs() < 0.2, "{a:?}");
        assert_eq!(ends(&curves[2]), (P::new(0.0, 15.0), P::new(60.0, 15.2)));
        assert_eq!(ends(&curves[3]).0.y, 30.0);
        assert_eq!(ends(&curves[4]).0.y, 31.5);
        assert_eq!(guides.found.iter().filter(|g| g.kind == GuideKind::Align).count(), 1);
    }
}
