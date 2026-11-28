//! Random scenes with a known ground truth.

use super::{Geom, Rgb, Scene, Shape};
use crate::num::math::sin_cos;
use crate::num::rng::Pcg32;
use std::f64::consts::PI;

/// What a random scene may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Polygon,
    Rounded,
    Circle,
    Ellipse,
    Stroke,
}

pub const ALL_KINDS: [Kind; 5] = [Kind::Polygon, Kind::Rounded, Kind::Circle, Kind::Ellipse, Kind::Stroke];

fn color(rng: &mut Pcg32) -> Rgb {
    [rng.next_f64(), rng.next_f64(), rng.next_f64()]
}

/// A colour far enough from `other` (sRGB distance ≥ 0.35) to give a clear edge.
fn contrasting(rng: &mut Pcg32, other: Rgb) -> Rgb {
    loop {
        let c = color(rng);
        let d: f64 = (0..3).map(|i| (c[i] - other[i]) * (c[i] - other[i])).sum();
        if d.sqrt() >= 0.35 {
            return c;
        }
    }
}

/// A convex polygon of `n` vertices around `center`, radius between `r/2` and `r`.
fn star_convex(rng: &mut Pcg32, center: (f64, f64), r: f64, n: u32) -> Vec<(f64, f64)> {
    let start = rng.range(0.0, 2.0 * PI);
    (0..n)
        .map(|k| {
            let a = start + 2.0 * PI * (f64::from(k) + rng.range(-0.3, 0.3)) / f64::from(n);
            let rr = rng.range(0.6 * r, r);
            let (s, c) = sin_cos(a);
            (center.0 + rr * c, center.1 + rr * s)
        })
        .collect()
}

/// A scene of `count` random shapes of the given kinds on a solid background.
pub fn scene(seed: u64, width: u32, height: u32, count: u32, kinds: &[Kind]) -> Scene {
    let mut rng = Pcg32::new(seed, 0x0073_6365_6e65);
    let background = color(&mut rng);
    let mut scene = Scene::new(width, height, background);
    let size = f64::from(width.min(height));
    for _ in 0..count {
        let kind = kinds[rng.below(kinds.len() as u32) as usize];
        let center = (rng.range(0.2, 0.8) * f64::from(width), rng.range(0.2, 0.8) * f64::from(height));
        let r = rng.range(0.08, 0.3) * size;
        let geom = match kind {
            Kind::Polygon => {
                let n = 3 + rng.below(5);
                Geom::Polygon(star_convex(&mut rng, center, r, n))
            }
            Kind::Rounded => {
                let n = 3 + rng.below(4);
                let vertices = star_convex(&mut rng, center, r, n);
                let radii = vertices
                    .iter()
                    .map(|_| if rng.below(3) == 0 { 0.0 } else { rng.range(1.0, 0.3 * r) })
                    .collect();
                Geom::Rounded { vertices, radii }
            }
            Kind::Circle => Geom::Circle { center, r },
            Kind::Ellipse => {
                Geom::Ellipse { center, a: r, b: r * rng.range(0.3, 0.9), angle: rng.range(0.0, PI) }
            }
            Kind::Stroke => {
                let (s, c) = sin_cos(rng.range(0.0, PI));
                Geom::Stroke {
                    from: (center.0 - r * c, center.1 - r * s),
                    to: (center.0 + r * c, center.1 + r * s),
                    width: rng.range(0.75, 4.0),
                }
            }
        };
        let fill = contrasting(&mut rng, background);
        scene.shapes.push(Shape::solid(geom, fill));
    }
    scene
}
