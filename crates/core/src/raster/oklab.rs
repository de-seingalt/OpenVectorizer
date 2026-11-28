//! Linear-light and Oklab planes, and the Jacobian of the conversion (docs/ALGORITHME.md §4.2,
//! §4.3). Standard Oklab (Ottosson 2020, MIT): unlike `crate::color`, no linear toe, so that the
//! noise propagation below is the exact derivative of the conversion used for detection.

use super::Plane;
use crate::RgbaImage;
use crate::num::math::cbrt;
use std::sync::OnceLock;

const M1: [[f64; 3]; 3] = [
    [0.412_221_470_8, 0.536_332_536_3, 0.051_445_992_9],
    [0.211_903_498_2, 0.680_699_545_1, 0.107_396_956_6],
    [0.088_302_461_9, 0.281_718_837_6, 0.629_978_700_5],
];
const M2: [[f64; 3]; 3] = [
    [0.210_454_255_3, 0.793_617_785_0, -0.004_072_046_8],
    [1.977_998_495_1, -2.428_592_205_0, 0.450_593_709_9],
    [0.025_904_037_1, 0.782_771_766_2, -0.808_675_766_0],
];

/// LMS values are bounded below before differentiating: the cube root has an infinite slope
/// at 0 (§4.3).
pub const LMS_FLOOR: f64 = 1e-4;

/// sRGB decoding of a gamma-encoded value in [0, 1].
pub fn decode(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { crate::num::math::pow((c + 0.055) / 1.055, 2.4) }
}

/// Derivative of [`decode`] with respect to the encoded value.
pub fn decode_slope(c: f64) -> f64 {
    if c <= 0.040_45 { 1.0 / 12.92 } else { 2.4 / 1.055 * crate::num::math::pow((c + 0.055) / 1.055, 1.4) }
}

/// Linear value of each 8-bit code.
pub fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = decode(i as f64 / 255.0) as f32;
        }
        t
    })
}

/// R, G, B planes in linear light, in [0, 1].
///
/// # Panics
/// Never: the planes have the image's size.
pub fn linear_planes(image: &RgbaImage) -> [Plane<f32>; 3] {
    let lut = linear_lut();
    let (w, h) = (image.width(), image.height());
    let px = image.data().as_chunks::<4>().0;
    [0, 1, 2].map(|c| {
        Plane::from_vec(w, h, px.iter().map(|p| lut[p[c] as usize]).collect())
            .expect("the planes have the image's size")
    })
}

/// R, G, B planes of the encoded values (codes / 255).
///
/// # Panics
/// Never: the planes have the image's size.
pub fn encoded_planes(image: &RgbaImage) -> [Plane<f32>; 3] {
    let (w, h) = (image.width(), image.height());
    let px = image.data().as_chunks::<4>().0;
    [0, 1, 2].map(|c| {
        Plane::from_vec(w, h, px.iter().map(|p| f32::from(p[c]) / 255.0).collect())
            .expect("the planes have the image's size")
    })
}

/// Alpha plane in [0, 1].
///
/// # Panics
/// Never: the plane has the image's size.
pub fn alpha_plane(image: &RgbaImage) -> Plane<f32> {
    let px = image.data().as_chunks::<4>().0;
    Plane::from_vec(image.width(), image.height(), px.iter().map(|p| f32::from(p[3]) / 255.0).collect())
        .expect("the planes have the image's size")
}

fn mul(m: &[[f64; 3]; 3], v: [f64; 3]) -> [f64; 3] {
    [0, 1, 2].map(|i| m[i][0] * v[0] + m[i][1] * v[1] + m[i][2] * v[2])
}

/// Oklab (L, a, b) of a linear RGB colour.
pub fn to_oklab(rgb: [f64; 3]) -> [f64; 3] {
    let lms = mul(&M1, rgb);
    mul(&M2, lms.map(cbrt))
}

/// Jacobian ∂(L, a, b)/∂(R, G, B) at a linear RGB colour: M2 · diag(1/(3·lms^{2/3})) · M1.
pub fn jacobian(rgb: [f64; 3]) -> [[f64; 3]; 3] {
    let lms = mul(&M1, rgb).map(|v| v.max(LMS_FLOOR));
    let slope = lms.map(|v| {
        let c = cbrt(v);
        1.0 / (3.0 * c * c)
    });
    let mut j = [[0.0; 3]; 3];
    for (i, row) in j.iter_mut().enumerate() {
        for (k, out) in row.iter_mut().enumerate() {
            *out = (0..3).map(|m| M2[i][m] * slope[m] * M1[m][k]).sum();
        }
    }
    j
}

/// L, a, b planes from linear planes.
///
/// # Panics
/// If the three planes differ in size.
pub fn oklab_planes(linear: &[Plane<f32>; 3]) -> [Plane<f32>; 3] {
    let (w, h) = (linear[0].width(), linear[0].height());
    let n = w as usize * h as usize;
    let mut out = [Vec::with_capacity(n), Vec::with_capacity(n), Vec::with_capacity(n)];
    for i in 0..n {
        let lab = to_oklab([0, 1, 2].map(|c| f64::from(linear[c].data()[i])));
        for (o, v) in out.iter_mut().zip(lab) {
            o.push(v as f32);
        }
    }
    out.map(|v| Plane::from_vec(w, h, v).expect("the planes have the image's size"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn white_is_l1_and_the_jacobian_matches_finite_differences() {
        let white = to_oklab([1.0; 3]);
        assert!((white[0] - 1.0).abs() < 1e-6 && white[1].abs() < 1e-6 && white[2].abs() < 1e-6, "{white:?}");
        let c = [0.2, 0.5, 0.05];
        let j = jacobian(c);
        let h = 1e-6;
        for k in 0..3 {
            let mut p = c;
            p[k] += h;
            let mut m = c;
            m[k] -= h;
            let (a, b) = (to_oklab(p), to_oklab(m));
            for i in 0..3 {
                let fd = (a[i] - b[i]) / (2.0 * h);
                assert!((fd - j[i][k]).abs() < 1e-5 * (1.0 + fd.abs()), "{i}{k}: {fd} vs {}", j[i][k]);
            }
        }
        // Encoding round trip of the decode slope.
        let (c, d) = (0.5, 1e-7);
        let fd = (decode(c + d) - decode(c - d)) / (2.0 * d);
        assert!((fd - decode_slope(c)).abs() < 1e-6);
    }
}
