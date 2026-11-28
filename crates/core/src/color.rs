//! sRGB ⇄ Oklab conversions. Oklab is perceptually uniform: Euclidean distance × 100 ≈ ΔE.
//!
//! One change from standard Oklab: like CIELAB, the cube root gets a linear toe below
//! (6/29)³. The plain cube root has an infinite slope at zero, so near-black colors that are
//! indistinguishable (`#000000` and `#040c03`) end up 15 ΔE apart and never merge.

use crate::num::math::{cbrt, pow};
use std::sync::OnceLock;

/// A color in Oklab (L in 0..=1, a/b roughly in -0.4..=0.4).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Lab {
    pub fn dist2(self, o: Lab) -> f32 {
        let (dl, da, db) = (self.l - o.l, self.a - o.a, self.b - o.b);
        dl * dl + da * da + db * db
    }

    pub fn scale(self, k: f32) -> Lab {
        Lab { l: self.l * k, a: self.a * k, b: self.b * k }
    }

    pub fn add(self, o: Lab) -> Lab {
        Lab { l: self.l + o.l, a: self.a + o.a, b: self.b + o.b }
    }
}

/// sRGB decoding of a gamma-encoded value in [0, 1] (exact formula, deterministic `pow`).
fn decode(c: f64) -> f64 {
    if c <= 0.040_45 { c / 12.92 } else { pow((c + 0.055) / 1.055, 2.4) }
}

fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            *v = decode(i as f64 / 255.0) as f32;
        }
        t
    })
}

/// Linear values halfway between consecutive codes: code k is the number of thresholds ≤ v.
/// Encoding by search gives the correctly rounded code without any `pow`.
fn encode_thresholds() -> &'static [f32; 255] {
    static T: OnceLock<[f32; 255]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [0.0; 255];
        for (k, v) in t.iter_mut().enumerate() {
            *v = decode((k as f64 + 0.5) / 255.0) as f32;
        }
        t
    })
}

/// 8-bit sRGB code of a linear value (clamped to [0, 1]).
pub fn encode_u8(v: f32) -> u8 {
    encode_thresholds().partition_point(|&t| t <= v) as u8
}

const DELTA: f32 = 6.0 / 29.0;

fn toe(t: f32) -> f32 {
    if t > DELTA * DELTA * DELTA { cbrt(f64::from(t)) as f32 } else { t / (3.0 * DELTA * DELTA) + 4.0 / 29.0 }
}

fn toe_inv(y: f32) -> f32 {
    if y > DELTA { y * y * y } else { (y - 4.0 / 29.0) * 3.0 * DELTA * DELTA }
}

pub fn srgb_to_oklab(rgb: [u8; 3]) -> Lab {
    let lut = linear_lut();
    let (r, g, b) = (lut[rgb[0] as usize], lut[rgb[1] as usize], lut[rgb[2] as usize]);
    let l = toe(0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b);
    let m = toe(0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b);
    let s = toe(0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b);
    Lab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

pub fn oklab_to_srgb(c: Lab) -> [u8; 3] {
    let l = toe_inv(c.l + 0.396_337_78 * c.a + 0.215_803_76 * c.b);
    let m = toe_inv(c.l - 0.105_561_346 * c.a - 0.063_854_17 * c.b);
    let s = toe_inv(c.l - 0.089_484_18 * c.a - 1.291_485_5 * c.b);
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    [encode_u8(r), encode_u8(g), encode_u8(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for rgb in
            [[0, 0, 0], [255, 255, 255], [220, 20, 20], [12, 200, 90], [128, 128, 128], [4, 12, 3], [1, 0, 2]]
        {
            assert_eq!(oklab_to_srgb(srgb_to_oklab(rgb)), rgb);
        }
    }

    #[test]
    fn near_blacks_are_close() {
        let d = srgb_to_oklab([0, 0, 0]).dist2(srgb_to_oklab([4, 12, 3])).sqrt();
        assert!(d < 0.04, "ΔE {}", d * 100.0);
    }
}
