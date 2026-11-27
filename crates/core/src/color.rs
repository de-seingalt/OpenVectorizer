//! sRGB ⇄ Oklab conversions. Oklab is perceptually uniform: Euclidean distance × 100 ≈ ΔE.

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

fn linear_lut() -> &'static [f32; 256] {
    static LUT: OnceLock<[f32; 256]> = OnceLock::new();
    LUT.get_or_init(|| {
        let mut t = [0.0; 256];
        for (i, v) in t.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.040_45 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        t
    })
}

pub fn srgb_to_oklab(rgb: [u8; 3]) -> Lab {
    let lut = linear_lut();
    let (r, g, b) = (lut[rgb[0] as usize], lut[rgb[1] as usize], lut[rgb[2] as usize]);
    let l = (0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b).cbrt();
    let m = (0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b).cbrt();
    let s = (0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b).cbrt();
    Lab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

pub fn oklab_to_srgb(c: Lab) -> [u8; 3] {
    let l = (c.l + 0.396_337_78 * c.a + 0.215_803_76 * c.b).powi(3);
    let m = (c.l - 0.105_561_346 * c.a - 0.063_854_17 * c.b).powi(3);
    let s = (c.l - 0.089_484_18 * c.a - 1.291_485_5 * c.b).powi(3);
    let r = 4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s;
    let g = -1.268_438 * l + 2.609_757_4 * m - 0.341_319_4 * s;
    let b = -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s;
    let enc = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        let e = if v <= 0.003_130_8 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
        (e * 255.0).round() as u8
    };
    [enc(r), enc(g), enc(b)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        for rgb in [[0, 0, 0], [255, 255, 255], [220, 20, 20], [12, 200, 90], [128, 128, 128]] {
            assert_eq!(oklab_to_srgb(srgb_to_oklab(rgb)), rgb);
        }
    }
}
