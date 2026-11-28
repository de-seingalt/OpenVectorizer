//! One channel of an image, stored row by row (planar layout, docs/ALGORITHME.md §14.4).

/// A `width × height` grid of values. Pixel (x, y) covers the square [x, x+1] × [y, y+1].
#[derive(Debug, Clone, PartialEq)]
pub struct Plane<T> {
    width: u32,
    height: u32,
    data: Vec<T>,
}

impl<T: Copy> Plane<T> {
    pub fn new(width: u32, height: u32, fill: T) -> Self {
        Plane { width, height, data: vec![fill; width as usize * height as usize] }
    }

    /// Plane from row-major values; `None` if the length does not match.
    pub fn from_vec(width: u32, height: u32, data: Vec<T>) -> Option<Self> {
        (data.len() == width as usize * height as usize).then_some(Plane { width, height, data })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn data(&self) -> &[T] {
        &self.data
    }

    pub fn data_mut(&mut self) -> &mut [T] {
        &mut self.data
    }

    pub fn into_vec(self) -> Vec<T> {
        self.data
    }

    #[inline]
    fn index(&self, x: u32, y: u32) -> usize {
        debug_assert!(x < self.width && y < self.height);
        y as usize * self.width as usize + x as usize
    }

    #[inline]
    pub fn get(&self, x: u32, y: u32) -> T {
        self.data[self.index(x, y)]
    }

    #[inline]
    pub fn set(&mut self, x: u32, y: u32, v: T) {
        let i = self.index(x, y);
        self.data[i] = v;
    }

    /// Value at (x, y) with coordinates mirrored at the borders (half-sample symmetric).
    #[inline]
    pub fn get_mirrored(&self, x: i64, y: i64) -> T {
        self.get(mirror(x, self.width), mirror(y, self.height))
    }

    pub fn row(&self, y: u32) -> &[T] {
        let w = self.width as usize;
        &self.data[y as usize * w..][..w]
    }

    pub fn row_mut(&mut self, y: u32) -> &mut [T] {
        let w = self.width as usize;
        &mut self.data[y as usize * w..][..w]
    }

    /// New plane with `f` applied to every value.
    pub fn map<U: Copy>(&self, f: impl FnMut(T) -> U) -> Plane<U> {
        Plane { width: self.width, height: self.height, data: self.data.iter().copied().map(f).collect() }
    }
}

/// Half-sample symmetric reflection of `i` into 0..n (n > 0): …, 1, 0 | 0, 1, …, n−1 | n−1, ….
#[inline]
pub fn mirror(i: i64, n: u32) -> u32 {
    let n = i64::from(n);
    let period = 2 * n;
    let m = i.rem_euclid(period);
    (if m < n { m } else { period - 1 - m }) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_and_mirroring() {
        let mut p = Plane::new(3, 2, 0u8);
        p.set(2, 1, 7);
        assert_eq!(p.row(1), &[0, 0, 7]);
        assert_eq!(p.get_mirrored(3, 1), 7);
        assert_eq!(p.get_mirrored(-1, 0), p.get(0, 0));
        let m: Vec<u32> = (-4..7).map(|i| mirror(i, 3)).collect();
        assert_eq!(m, [2, 2, 1, 0, 0, 1, 2, 2, 1, 0, 0]);
        assert!(Plane::from_vec(2, 2, vec![0; 3]).is_none());
        assert_eq!(p.map(u32::from).get(2, 1), 7);
    }
}
