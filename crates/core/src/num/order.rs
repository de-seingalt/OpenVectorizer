//! Deterministic ordering of floating-point data (docs/ALGORITHME.md §14.4).
//!
//! Rules: compare floats with `total_cmp`, never sort on a key that can tie unless the sort is
//! stable (equal keys then keep their input order, which is itself deterministic), and keep NaN
//! out of sort keys.

use std::cmp::Ordering;

/// Total order on `f64` that refuses NaN in debug builds.
#[inline]
pub fn cmp_f64(a: f64, b: f64) -> Ordering {
    debug_assert!(!a.is_nan() && !b.is_nan(), "NaN in a sort key");
    a.total_cmp(&b)
}

/// Stable sort by a floating-point key: equal keys keep their input order.
pub fn sort_by_f64<T>(items: &mut [T], mut key: impl FnMut(&T) -> f64) {
    items.sort_by(|a, b| cmp_f64(key(a), key(b)));
}

/// Indices of `keys` in increasing key order; ties are broken by index.
pub fn argsort_f64(keys: &[f64]) -> Vec<u32> {
    let mut idx: Vec<u32> = (0..keys.len() as u32).collect();
    idx.sort_by(|&a, &b| cmp_f64(keys[a as usize], keys[b as usize]).then(a.cmp(&b)));
    idx
}

/// Index of the smallest key, the first one on ties. `None` for an empty slice.
pub fn argmin_f64(keys: impl IntoIterator<Item = f64>) -> Option<u32> {
    let mut best: Option<(u32, f64)> = None;
    for (i, k) in keys.into_iter().enumerate() {
        if best.is_none_or(|(_, b)| cmp_f64(k, b) == Ordering::Less) {
            best = Some((i as u32, k));
        }
    }
    best.map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ties_keep_input_order() {
        let mut v = [(1.0, 'a'), (0.5, 'b'), (1.0, 'c'), (-0.0, 'd'), (0.0, 'e')];
        sort_by_f64(&mut v, |p| p.0);
        let order: String = v.iter().map(|p| p.1).collect();
        // total_cmp puts -0 before +0.
        assert_eq!(order, "debac");
        assert_eq!(argsort_f64(&[2.0, 1.0, 2.0, 1.0]), [1, 3, 0, 2]);
        assert_eq!(argmin_f64([3.0, 1.0, 1.0]), Some(1));
        assert_eq!(argmin_f64(std::iter::empty()), None);
    }
}
