//! Guides: the geometric patterns found on the boundaries (straight lines, tangent arcs,
//! circles, ellipses, symmetries). The tracing follows them; the user can see them and turn
//! each one off. A guide is identified by its kind and a key point, found again at every
//! tracing (like corner edits).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::fit::P;

/// What a guide is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum GuideKind {
    /// A straight part of a boundary (key: its middle).
    Line,
    /// An arc tangent to two straight parts (key: its middle).
    Arc,
    /// A whole circle (key: its center).
    Circle,
    /// A whole ellipse (key: its center).
    Ellipse,
    /// Mirror axes and/or rotation of a shape (key: its center).
    Symmetry,
}

/// A guide turned off by the user.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GuideKey {
    pub kind: GuideKind,
    pub x: f32,
    pub y: f32,
}

/// How far (px) a disabled key looks for its guide.
pub const GUIDE_REACH: f64 = 2.0;

/// A guide of the result, for the editor.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Guide {
    pub kind: GuideKind,
    /// Key point (see [`GuideKind`]).
    pub x: f32,
    pub y: f32,
    /// The tracing follows it (false: turned off by the user).
    pub enabled: bool,
    /// SVG path data drawing the guide (image px).
    pub d: String,
}

/// Guides turned off, and guides found while tracing.
#[derive(Debug, Clone, Default)]
pub struct Guides {
    pub disabled: Vec<GuideKey>,
    pub found: Vec<Guide>,
}

impl Guides {
    pub fn new(disabled: &[GuideKey]) -> Self {
        Guides { disabled: disabled.to_vec(), found: Vec::new() }
    }

    /// Whether the guide of `kind` at `key` is turned off.
    pub fn is_disabled(&self, kind: GuideKind, key: P) -> bool {
        self.disabled
            .iter()
            .any(|k| k.kind == kind && (P::new(f64::from(k.x), f64::from(k.y)) - key).len() <= GUIDE_REACH)
    }

    /// Record a guide; returns whether it is enabled (the tracing may follow it).
    pub fn add(&mut self, kind: GuideKind, key: P, d: String) -> bool {
        let enabled = !self.is_disabled(kind, key);
        self.found.push(Guide { kind, x: key.x as f32, y: key.y as f32, enabled, d });
        enabled
    }
}

fn num(out: &mut String, v: f64) {
    let r = (v * 100.0).round() / 100.0;
    let r = if r == 0.0 { 0.0 } else { r };
    let _ = write!(out, "{r}");
}

fn pt(out: &mut String, p: P) {
    num(out, p.x);
    out.push(' ');
    num(out, p.y);
}

/// Path data of a segment.
pub fn segment(a: P, b: P) -> String {
    let mut d = String::from("M");
    pt(&mut d, a);
    d.push('L');
    pt(&mut d, b);
    d
}

/// Path data of an arc of radius `r` from `a` to `b` (`sweep`: SVG sweep flag; less than 180°).
pub fn arc(a: P, b: P, r: f64, sweep: bool) -> String {
    let mut d = String::from("M");
    pt(&mut d, a);
    d.push('A');
    num(&mut d, r);
    d.push(' ');
    num(&mut d, r);
    let _ = write!(d, " 0 0 {} ", u8::from(sweep));
    pt(&mut d, b);
    d
}

/// Path data of a full ellipse (a circle when `a == b`), `angle` in radians.
pub fn ellipse(center: P, a: f64, b: f64, angle: f64) -> String {
    let (s, c) = angle.sin_cos();
    let p0 = center + P::new(a * c, a * s);
    let p1 = center - P::new(a * c, a * s);
    let mut d = String::from("M");
    pt(&mut d, p0);
    for p in [p1, p0] {
        d.push('A');
        num(&mut d, a);
        d.push(' ');
        num(&mut d, b);
        d.push(' ');
        num(&mut d, angle.to_degrees());
        d.push_str(" 0 1 ");
        pt(&mut d, p);
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_guides_are_found_again_by_kind_and_position() {
        let off = [GuideKey { kind: GuideKind::Circle, x: 10.0, y: 10.0 }];
        let mut guides = Guides::new(&off);
        // Slightly moved (the tracing changed a little): still off.
        assert!(!guides.add(
            GuideKind::Circle,
            P::new(11.0, 10.5),
            ellipse(P::new(11.0, 10.5), 5.0, 5.0, 0.0)
        ));
        // Other kind or too far: on.
        assert!(guides.add(GuideKind::Ellipse, P::new(10.0, 10.0), String::new()));
        assert!(guides.add(GuideKind::Circle, P::new(14.0, 10.0), String::new()));
        assert_eq!(guides.found.iter().filter(|g| g.enabled).count(), 2);
        assert!(guides.found[0].d.starts_with("M16 10.5A5 5 0 0 1 6 10.5"), "{}", guides.found[0].d);
    }
}
