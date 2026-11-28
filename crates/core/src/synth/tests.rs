use super::degrade::{blur, noise, upscale_bilinear, upscale_nearest};
use super::random::{ALL_KINDS, scene};
use super::*;
use std::f64::consts::PI;

fn area(geom: &Geom, w: u32, h: u32) -> f64 {
    let mut c = Coverage::new(w, h);
    for contour in geom.contours(FLATTEN_TOLERANCE) {
        c.contour(&contour);
    }
    c.finish().data().iter().sum()
}

fn close(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

#[test]
fn rendered_areas_match_the_exact_geometry() {
    let circle = Geom::Circle { center: (20.3, 19.6), r: 10.2 };
    assert!(close(area(&circle, 40, 40), PI * 10.2 * 10.2, 0.01));
    let ellipse = Geom::Ellipse { center: (30.0, 20.0), a: 18.0, b: 7.5, angle: 0.4 };
    assert!(close(area(&ellipse, 60, 40), PI * 18.0 * 7.5, 0.01));
    let square = vec![(5.0, 5.0), (25.0, 5.0), (25.0, 25.0), (5.0, 25.0)];
    let rounded = Geom::Rounded { vertices: square.clone(), radii: vec![5.0; 4] };
    assert!(close(area(&rounded, 30, 30), 400.0 - (4.0 - PI) * 25.0, 0.01));
    // Reversed vertex order: same shape.
    let reversed = Geom::Rounded { vertices: square.into_iter().rev().collect(), radii: vec![5.0; 4] };
    assert!(close(area(&reversed, 30, 30), 400.0 - (4.0 - PI) * 25.0, 0.01));
    let stroke = Geom::Stroke { from: (2.0, 3.0), to: (30.0, 17.0), width: 1.5 };
    assert!(close(area(&stroke, 32, 20), 1.5 * (28.0f64 * 28.0 + 14.0 * 14.0).sqrt(), 1e-9));
    // A Bézier circle approximation (k = 0.5523), within 0.03 % of the circle's area.
    let (c, r, k) = ((16.0, 16.0), 10.0, 0.552_284_75 * 10.0);
    let q = [(c.0 + r, c.1), (c.0, c.1 + r), (c.0 - r, c.1), (c.0, c.1 - r), (c.0 + r, c.1)];
    let h = [(0.0, k), (-k, 0.0), (0.0, -k), (k, 0.0)];
    let pieces = (0..4)
        .map(|i| {
            let (a, b) = (q[i], q[i + 1]);
            [a, (a.0 + h[i].0, a.1 + h[i].1), (b.0 - h[(i + 1) % 4].0, b.1 - h[(i + 1) % 4].1), b]
        })
        .collect();
    assert!(close(area(&Geom::Bezier(pieces), 32, 32), PI * 100.0, 0.1));
}

#[test]
fn the_ground_truth_lists_corners_and_segments() {
    let square = vec![(5.0, 5.0), (25.0, 5.0), (25.0, 25.0), (5.0, 25.0)];
    let f = Geom::Rounded { vertices: square, radii: vec![5.0, 0.0, 5.0, 0.0] }.features();
    let corners: Vec<_> = f
        .iter()
        .filter_map(|x| {
            if let Feature::Corner { angle, radius, .. } = x { Some((*angle, *radius)) } else { None }
        })
        .collect();
    assert_eq!(corners.len(), 4);
    assert!(corners.iter().all(|(a, _)| close(*a, PI / 2.0, 1e-12)));
    assert_eq!(corners.iter().map(|c| c.1).collect::<Vec<_>>(), [5.0, 0.0, 5.0, 0.0]);
    let lengths: Vec<f64> = f
        .iter()
        .filter_map(|x| {
            if let Feature::Segment { from, to } = x {
                Some(((to.0 - from.0) * (to.0 - from.0) + (to.1 - from.1) * (to.1 - from.1)).sqrt())
            } else {
                None
            }
        })
        .collect();
    assert!(lengths.iter().all(|&l| close(l, 15.0, 1e-9)), "{lengths:?}");
    // A reflex vertex has an interior angle above π.
    let l_shape = vec![(0.0, 0.0), (20.0, 0.0), (20.0, 10.0), (10.0, 10.0), (10.0, 20.0), (0.0, 20.0)];
    let reflex = Geom::Polygon(l_shape)
        .features()
        .iter()
        .filter(|x| matches!(x, Feature::Corner { angle, .. } if *angle > PI))
        .count();
    assert_eq!(reflex, 1);
}

#[test]
fn scenes_render_shape_by_shape() {
    let red = [1.0, 0.0, 0.0];
    let s = Scene::new(20, 20, [0.0, 0.0, 1.0])
        .with(Shape::solid(Geom::Circle { center: (10.0, 10.0), r: 6.5 }, red));
    let img = s.image();
    let px = |x: u32, y: u32| &img.data()[((y * 20 + x) * 4) as usize..][..4];
    assert_eq!(px(10, 10), &[255, 0, 0, 255]);
    assert_eq!(px(0, 0), &[0, 0, 255, 255]);
    let edge = px(16, 10); // pixel [16, 17] half covered at y ≈ 10.5
    assert!(edge[0] > 60 && edge[0] < 200 && edge[2] > 60, "{edge:?}");
    // Transparent background: the shape's edge is in the alpha channel, colour stays red.
    let mut t = s.clone();
    t.background_alpha = 0.0;
    let planes = t.render();
    assert!((planes[0].get(16, 10) - 1.0).abs() < 1e-12);
    assert!(planes[3].get(16, 10) > 0.2 && planes[3].get(16, 10) < 0.8);
    // Linear-light blending makes the same edge brighter.
    let mut lin = s.clone();
    lin.blend = Blend::Linear;
    assert!(lin.render()[0].get(16, 10) > s.render()[0].get(16, 10));
    // A gradient fill.
    let g = Scene::new(10, 1, [0.0; 3]).with(Shape {
        geom: Geom::Polygon(vec![(0.0, 0.0), (10.0, 0.0), (10.0, 1.0), (0.0, 1.0)]),
        fill: Fill::Linear { from: (0.0, 0.5), to: (10.0, 0.5), c0: [0.0; 3], c1: [1.0; 3] },
        opacity: 1.0,
    });
    let r = &g.render()[0];
    assert!(close(r.get(0, 0), 0.05, 1e-12) && close(r.get(9, 0), 0.95, 1e-12));
}

#[test]
fn degradations_are_deterministic_and_keep_their_promises() {
    let base = scene(7, 64, 48, 6, &ALL_KINDS).render();
    assert_eq!(blur(&base, 1.0), blur(&base, 1.0));
    // Blur keeps a constant image constant.
    let flat = Scene::new(16, 16, [0.3, 0.6, 0.9]).render();
    assert!(blur(&flat, 1.5)[1].data().iter().all(|&v| close(v, 0.6, 1e-12)));
    let n = noise(&flat, 0.01, 3);
    let d: Vec<f64> = n[0].data().iter().map(|v| v - 0.3).collect();
    let sd = (d.iter().map(|v| v * v).sum::<f64>() / d.len() as f64).sqrt();
    assert!(close(sd, 0.01, 0.002), "{sd}");
    assert_ne!(noise(&flat, 0.01, 3), noise(&flat, 0.01, 4));
    let up = upscale_nearest(&base, 3);
    assert_eq!((up[0].width(), up[0].height()), (192, 144));
    assert_eq!(up[2].get(5, 7), base[2].get(1, 2));
    assert!(upscale_bilinear(&flat, 2)[2].data().iter().all(|&v| close(v, 0.9, 1e-12)));
}

#[test]
fn random_scenes_are_reproducible() {
    let a = scene(42, 80, 60, 5, &ALL_KINDS);
    assert_eq!(a, scene(42, 80, 60, 5, &ALL_KINDS));
    assert_ne!(a, scene(43, 80, 60, 5, &ALL_KINDS));
    assert_eq!(a.shapes.len(), 5);
    assert_ne!(a.features().len(), 0);
}
