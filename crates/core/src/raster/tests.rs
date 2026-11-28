//! Phase P1 exit tests (docs/ALGORITHME.md §16): filters against analytic references, noise
//! estimates against known noise, pyramid geometry, input classification.

use super::classify::classify;
use super::gauss::{Kernel, derivatives, smooth};
use super::noise::NoiseModel;
use super::oklab::{alpha_plane, decode_slope, encoded_planes};
use super::pyramid::Pyramid;
use super::{Coverage, Plane};
use crate::num::math::{exp, sin};
use crate::num::rng::Pcg32;
use crate::synth::degrade::{amplify_bottom_half, gaussian, noise_linear, upscale_nearest};
use crate::synth::{Fill, Geom, Scene, Shape, quantize};

fn plane(w: u32, h: u32, f: impl Fn(f64, f64) -> f64) -> Plane<f32> {
    let mut p = Plane::new(w, h, 0.0);
    for y in 0..h {
        for x in 0..w {
            p.set(x, y, f(f64::from(x) + 0.5, f64::from(y) + 0.5) as f32);
        }
    }
    p
}

fn rel(a: f64, b: f64) -> f64 {
    (a - b).abs() / b.abs().max(1e-12)
}

#[test]
fn kernels_are_normalised() {
    for sigma in [0.5, 0.6, 0.7, 0.8, 1.0, 1.5, 2.5] {
        let k = Kernel::new(sigma);
        let sum: f64 = f64::from(k.g[0]) + 2.0 * k.g[1..].iter().map(|&v| f64::from(v)).sum::<f64>();
        assert!((sum - 1.0).abs() < 1e-6, "σ {sigma}: Σg = {sum}");
        // A ramp of slope 1 has derivative exactly 1: Σ 2j·d_j = 1.
        let slope: f64 = k.d.iter().enumerate().map(|(j, &v)| 2.0 * j as f64 * f64::from(v)).sum();
        assert!((slope - 1.0).abs() < 1e-6, "σ {sigma}: slope {slope}");
        if sigma < 0.8 {
            // Lindeberg's discrete kernel has variance exactly t = σ².
            let var: f64 =
                2.0 * k.g.iter().enumerate().map(|(j, &v)| (j * j) as f64 * f64::from(v)).sum::<f64>();
            assert!(rel(var, sigma * sigma) < 1e-5, "σ {sigma}: variance {var}");
        }
    }
}

#[test]
fn filters_match_the_analytic_gaussian() {
    // f = sin(ωx): smoothing multiplies by e^{−σ²ω²/2}, the derivative is ω·cos(ωx)·the same.
    let (w, sigma) = (0.4, 1.5);
    let src = plane(96, 40, |x, _| sin(w * x));
    let k = Kernel::new(sigma);
    let gain = exp(-sigma * sigma * w * w / 2.0);
    let smoothed = smooth(&src, &k);
    let mut checked = 0;
    derivatives(&src, &k, |y, s, dx, dy| {
        assert_eq!(s, smoothed.row(y), "same operations, same bits");
        if !(10..30).contains(&y) {
            return;
        }
        for x in 12..84u32 {
            let xc = f64::from(x) + 0.5;
            assert!((f64::from(s[x as usize]) - gain * sin(w * xc)).abs() < 2e-4, "smooth at {x}");
            let want = w * gain * crate::num::math::cos(w * xc);
            assert!((f64::from(dx[x as usize]) - want).abs() < 2e-4, "dx at {x}");
            assert!(dy[x as usize].abs() < 1e-5);
            checked += 1;
        }
    });
    assert_eq!(checked, 20 * 72);
}

#[test]
fn derivative_noise_matches_the_predicted_gain() {
    let mut rng = Pcg32::new(11, 1);
    let mut src = Plane::new(300, 300, 0.0f32);
    for v in src.data_mut() {
        *v = gaussian(&mut rng) as f32;
    }
    for sigma in [0.7, 1.0, 1.5] {
        let k = Kernel::new(sigma);
        let (mut sum, mut n) = (0.0, 0.0);
        derivatives(&src, &k, |y, _, dx, dy| {
            if (10..290).contains(&y) {
                for x in 10..290 {
                    sum += f64::from(dx[x]) * f64::from(dx[x]) + f64::from(dy[x]) * f64::from(dy[x]);
                    n += 2.0;
                }
            }
        });
        let var = sum / n;
        assert!(
            rel(var, k.derivative_noise_gain()) < 0.05,
            "σ {sigma}: {var} vs {}",
            k.derivative_noise_gain()
        );
    }
}

#[test]
fn the_pyramid_keeps_values_and_edge_positions() {
    let flat = Pyramid::new(plane(130, 70, |_, _| 0.25));
    assert_eq!(
        flat.levels.iter().map(|l| (l.width(), l.height())).collect::<Vec<_>>(),
        [(130, 70), (65, 35), (32, 17)]
    );
    assert!(flat.levels.iter().all(|l| l.data().iter().all(|&v| (v - 0.25).abs() < 1e-6)));
    // A vertical edge at x = 40.3 with exact coverage: its half-height crossing stays there.
    let mut c = Coverage::new(128, 64);
    c.contour(&[(0.0, 0.0), (40.3, 0.0), (40.3, 64.0), (0.0, 64.0)]);
    let base = c.finish().map(|v| v as f32);
    let pyr = Pyramid::new(base);
    for (o, level) in pyr.levels.iter().enumerate() {
        let row = level.row(level.height() / 2);
        // The area left of the edge is kept exactly by the blur.
        let area: f64 = row.iter().map(|&v| f64::from(v)).sum::<f64>() * f64::from(1u32 << o);
        assert!((area - 40.3).abs() < 1e-3, "level {o}: area {area}");
        if o == 0 {
            continue; // an unblurred edge is a box: interpolating between centres is meaningless
        }
        let i = row.iter().position(|&v| v < 0.5).expect("crossing");
        let (a, b) = (f64::from(row[i - 1]), f64::from(row[i]));
        let (xa, xb) = (Pyramid::center(o, i as u32 - 1), Pyramid::center(o, i as u32));
        let x = xa + (a - 0.5) / (a - b) * (xb - xa);
        assert!((x - 40.3).abs() < 0.05 * f64::from(1u32 << o), "level {o}: {x}");
    }
}

fn encode_v(v: f64) -> f64 {
    crate::synth::srgb_encode(v)
}

fn gradient_scene(w: u32, h: u32) -> Scene {
    let mut s = Scene::new(w, h, [0.0; 3]);
    s.shapes.push(Shape {
        geom: Geom::Polygon(vec![
            (0.0, 0.0),
            (f64::from(w), 0.0),
            (f64::from(w), f64::from(h)),
            (0.0, f64::from(h)),
        ]),
        fill: Fill::Linear { from: (0.0, 0.0), to: (f64::from(w), 0.0), c0: [0.02; 3], c1: [0.98; 3] },
        opacity: 1.0,
    });
    s.with(Shape::solid(Geom::Circle { center: (90.0, 80.0), r: 40.0 }, [0.9, 0.2, 0.1]))
}

fn noise_of(planes: &[Plane<f64>; 4]) -> NoiseModel {
    let img = quantize(planes);
    let info = classify(&img);
    assert!(!info.jpeg, "jpeg score {}", info.jpeg_score);
    NoiseModel::estimate(&encoded_planes(&img), None, info.jpeg)
}

#[test]
fn the_noise_level_function_is_recovered() {
    let clean = gradient_scene(320, 256).render();
    // Clean digital art: no noise beyond the floor.
    let model = noise_of(&clean);
    for lf in &model.channels {
        for v in [0.05, 0.3, 0.7] {
            assert!(rel(lf.sigma(v, 1.0), lf.floor_at(v)) < 1e-9, "{lf:?}");
        }
    }
    // Known signal-dependent noise in linear light.
    let (alpha, beta) = (4e-5, 2e-6);
    let noisy = noise_linear(&clean, alpha, beta, 5);
    let model = noise_of(&noisy);
    for lf in &model.channels {
        for v in [0.05, 0.2, 0.5, 0.8] {
            // 8-bit quantization adds its own noise, 1/√12 code, scaled by the decoding slope.
            let q = decode_slope(encode_v(v)) / 255.0 / 12f64.sqrt();
            let truth = (alpha * v + beta + q * q).sqrt();
            assert!(rel(lf.sigma(v, 1.0), truth) < 0.10, "at {v}: {} vs {truth} ({lf:?})", lf.sigma(v, 1.0));
        }
    }
    // Twice the noise in the bottom half (every intensity class spans both halves, so the level
    // function cannot absorb it): the local factors see it.
    let uneven = amplify_bottom_half(&noisy, &clean, 2.0);
    let model = noise_of(&uneven);
    let (top, bottom) = (model.factor(100.0, 32.0), model.factor(100.0, 224.0));
    assert!(rel(bottom / top, 2.0) < 0.25, "top {top}, bottom {bottom}");
}

#[test]
fn input_classes() {
    // Pixel art: a random 16×16 sprite enlarged ×4.
    let mut rng = Pcg32::new(3, 3);
    let mut sprite = Scene::new(16, 16, [0.0; 3]).render();
    for p in &mut sprite[..3] {
        for v in p.data_mut() {
            *v = f64::from(rng.below(4)) / 3.0;
        }
    }
    let info = classify(&quantize(&upscale_nearest(&sprite, 4)));
    assert_eq!(info.pixel_art.map(|g| g.cell), Some(4));
    assert!(info.small && !info.jpeg && !info.transparent);
    // A hard-edged (aliased) logo is not pixel art.
    let logo = Scene::new(200, 150, [1.0; 3])
        .with(Shape::solid(Geom::Circle { center: (70.0, 70.0), r: 45.3 }, [0.1, 0.3, 0.8]))
        .with(Shape::solid(
            Geom::Polygon(vec![(110.0, 20.0), (190.0, 60.0), (130.0, 140.0)]),
            [0.9, 0.1, 0.1],
        ))
        .render()
        .map(|p| p.map(f64::round));
    let info = classify(&quantize(&logo));
    assert_eq!(info.pixel_art, None);
    assert!(!info.small && !info.jpeg);
    // Transparency.
    let mut t = gradient_scene(64, 64);
    t.background_alpha = 0.0;
    t.shapes.remove(0);
    let img = t.image();
    assert!(classify(&img).transparent);
    let noise = NoiseModel::estimate(&encoded_planes(&img), Some(&alpha_plane(&img)), false);
    assert!(noise.alpha.is_some());
}

#[test]
fn uniform_noise_in_encoded_values_is_recovered() {
    // ±16 codes uniform, added to the 8-bit values like the benchmark's `noise` variant.
    let clean = quantize(&gradient_scene(320, 256).render());
    let mut rng = Pcg32::new(9, 9);
    let mut data = clean.data().to_vec();
    for (i, v) in data.iter_mut().enumerate() {
        if i % 4 != 3 {
            *v = (i32::from(*v) + rng.below(33) as i32 - 16).clamp(0, 255) as u8;
        }
    }
    let img = crate::RgbaImage::new(clean.width(), clean.height(), data).expect("image");
    let model = NoiseModel::estimate(&encoded_planes(&img), None, false);
    let sigma_codes = (33.0f64 * 33.0 - 1.0).sqrt() / 12f64.sqrt();
    for lf in &model.channels {
        for c in [0.3, 0.5, 0.7] {
            let v = super::oklab::decode(c);
            let est = lf.sigma(v, 1.0) / decode_slope(c) * 255.0;
            // The MAD factor 0.6745 is exact for Gaussian noise; uniform noise (short tails)
            // reads about 8 % high. Real image noise is close to Gaussian (see the test above).
            assert!(rel(est, sigma_codes) < 0.15, "at code {c}: {est} vs {sigma_codes}");
        }
    }
}
