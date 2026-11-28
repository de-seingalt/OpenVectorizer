//! Seeded pseudo-random numbers: PCG32 (O'Neill 2014, XSH-RR variant) and FNV-1a seeds.
//!
//! Never seeded from the system: a seed is derived from the image and the settings, so the
//! same input gives the same output everywhere (docs/ALGORITHME.md §14.4). All arithmetic
//! uses explicit `u32`/`u64`, never `usize`, whose width differs between native and WASM.

/// PCG32 generator: 64-bit state, 32-bit output.
#[derive(Debug, Clone)]
pub struct Pcg32 {
    state: u64,
    inc: u64,
}

const MULTIPLIER: u64 = 6_364_136_223_846_793_005;

impl Pcg32 {
    /// Generator for `seed` on the stream `stream` (two streams never overlap).
    pub fn new(seed: u64, stream: u64) -> Self {
        let mut rng = Pcg32 { state: 0, inc: (stream << 1) | 1 };
        rng.next_u32();
        rng.state = rng.state.wrapping_add(seed);
        rng.next_u32();
        rng
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(MULTIPLIER).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1) with 53 random bits.
    pub fn next_f64(&mut self) -> f64 {
        let hi = u64::from(self.next_u32() >> 5); // 27 bits
        let lo = u64::from(self.next_u32() >> 6); // 26 bits
        ((hi << 26) | lo) as f64 * (1.0 / 9_007_199_254_740_992.0)
    }

    /// Uniform in [lo, hi).
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform integer in 0..n, without modulo bias (Lemire's method).
    ///
    /// # Panics
    /// If `n` is 0.
    pub fn below(&mut self, n: u32) -> u32 {
        assert!(n > 0, "empty range");
        let threshold = n.wrapping_neg() % n;
        loop {
            let m = u64::from(self.next_u32()) * u64::from(n);
            if (m as u32) >= threshold {
                return (m >> 32) as u32;
            }
        }
    }
}

/// FNV-1a hash of bytes, to derive seeds from data.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_pcg32_reference_sequence() {
        // Reference output of pcg32_random_r for seed 42, stream 54 (pcg-c-basic demo).
        let mut rng = Pcg32::new(42, 54);
        let got: Vec<u32> = (0..6).map(|_| rng.next_u32()).collect();
        assert_eq!(got, [0xa15c_02b7, 0x7b47_f409, 0xba1d_3330, 0x83d2_f293, 0xbfa4_784b, 0xcbed_606e]);
    }

    #[test]
    fn floats_and_ranges_stay_in_bounds() {
        let mut rng = Pcg32::new(1, 2);
        for _ in 0..10_000 {
            let x = rng.next_f64();
            assert!((0.0..1.0).contains(&x));
            assert!(rng.below(7) < 7);
        }
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
