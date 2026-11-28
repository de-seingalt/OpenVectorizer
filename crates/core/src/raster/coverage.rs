//! Exact area coverage of filled polygons (non-zero winding, clamped).
//!
//! Each edge adds its signed area to an accumulation buffer, row by row; a running sum along
//! each row then gives the fraction of every pixel covered by the shape. This is the signed
//! area accumulation scheme of font rasterizers (as in R. Levien's `font-rs`, Apache-2.0),
//! re-implemented here in f64 with clipping to the image. Curves are flattened by the caller.
//!
//! Coordinates are in SVG space: pixel (x, y) covers [x, x+1] × [y, y+1].

use super::Plane;

/// Accumulates the edges of one shape.
#[derive(Debug, Clone)]
pub struct Coverage {
    width: u32,
    height: u32,
    /// `height` rows of `width + 2` cells.
    acc: Vec<f64>,
}

impl Coverage {
    pub fn new(width: u32, height: u32) -> Self {
        Coverage { width, height, acc: vec![0.0; (width as usize + 2) * height as usize] }
    }

    /// Add the closed contour through `points` (the last point joins the first). The winding
    /// direction does not matter for a single contour; holes must turn the other way.
    pub fn contour(&mut self, points: &[(f64, f64)]) {
        for (i, &a) in points.iter().enumerate() {
            let b = points[(i + 1) % points.len()];
            self.line(a, b);
        }
    }

    /// Add one directed edge.
    pub fn line(&mut self, a: (f64, f64), b: (f64, f64)) {
        let w = f64::from(self.width);
        // Clamping x keeps the winding of the parts left or right of the image, which is all
        // that matters for the coverage of the pixels inside it.
        let (a, b) = ((a.0.clamp(0.0, w), a.1), (b.0.clamp(0.0, w), b.1));
        if a.1.total_cmp(&b.1).is_eq() {
            return;
        }
        let (dir, p0, p1) = if a.1 < b.1 { (1.0, a, b) } else { (-1.0, b, a) };
        let dxdy = (p1.0 - p0.0) / (p1.1 - p0.1);
        let y_start = p0.1.max(0.0);
        let y_end = p1.1.min(f64::from(self.height));
        if y_start >= y_end {
            return;
        }
        let stride = self.width as usize + 2;
        let mut x = p0.0 + (y_start - p0.1) * dxdy;
        let mut y = y_start.floor();
        while y < y_end {
            let row = y as usize * stride;
            let top = y.max(y_start);
            let bottom = (y + 1.0).min(y_end);
            let dy = bottom - top;
            let x_next = (x + dxdy * dy).clamp(0.0, w);
            let d = dy * dir;
            let (x0, x1) = if x < x_next { (x, x_next) } else { (x_next, x) };
            let x0_floor = x0.floor();
            let x0i = x0_floor as usize;
            let x1_ceil = x1.ceil();
            let x1i = (x1_ceil as usize).max(x0i + 1);
            let cells = &mut self.acc[row..row + stride];
            if x1i <= x0i + 1 {
                // The edge stays within one column: split by the mean x.
                let xm = f64::midpoint(x, x_next) - x0_floor;
                cells[x0i] += d - d * xm;
                cells[x0i + 1] += d * xm;
            } else {
                let s = 1.0 / (x1 - x0);
                let x0f = x0 - x0_floor;
                let a0 = 0.5 * s * (1.0 - x0f) * (1.0 - x0f);
                let x1f = x1 - x1_ceil + 1.0;
                let am = 0.5 * s * x1f * x1f;
                cells[x0i] += d * a0;
                if x1i == x0i + 2 {
                    cells[x0i + 1] += d * (1.0 - a0 - am);
                } else {
                    let a1 = s * (1.5 - x0f);
                    cells[x0i + 1] += d * (a1 - a0);
                    for cell in &mut cells[x0i + 2..x1i - 1] {
                        *cell += d * s;
                    }
                    let a2 = a1 + (x1i - x0i - 3) as f64 * s;
                    cells[x1i - 1] += d * (1.0 - a2 - am);
                }
                cells[x1i] += d * am;
            }
            x = x_next;
            y += 1.0;
        }
    }

    /// Fraction of each pixel covered, in [0, 1].
    pub fn finish(&self) -> Plane<f64> {
        let stride = self.width as usize + 2;
        let mut out = Plane::new(self.width, self.height, 0.0);
        for y in 0..self.height {
            let cells = &self.acc[y as usize * stride..][..stride];
            let mut sum = 0.0;
            for (o, &c) in out.row_mut(y).iter_mut().zip(cells) {
                sum += c;
                *o = sum.abs().min(1.0);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn covered(points: &[(f64, f64)], w: u32, h: u32) -> Plane<f64> {
        let mut c = Coverage::new(w, h);
        c.contour(points);
        c.finish()
    }

    #[test]
    fn axis_aligned_squares_are_exact() {
        let p = covered(&[(1.0, 1.0), (3.0, 1.0), (3.0, 3.0), (1.0, 3.0)], 5, 5);
        assert_eq!(p.row(0), &[0.0; 5]);
        assert_eq!(p.row(1), &[0.0, 1.0, 1.0, 0.0, 0.0]);
        let half = covered(&[(0.5, 0.0), (2.0, 0.0), (2.0, 1.0), (0.5, 1.0)], 3, 1);
        assert_eq!(half.row(0), &[0.5, 1.0, 0.0]);
    }

    #[test]
    fn a_triangle_covers_its_area_and_clipping_keeps_the_inside() {
        // Right triangle, legs 4: area 8, half of each diagonal pixel.
        let p = covered(&[(0.0, 0.0), (4.0, 0.0), (0.0, 4.0)], 4, 4);
        let total: f64 = p.data().iter().sum();
        assert!((total - 8.0).abs() < 1e-12, "{total}");
        assert!((p.get(3, 0) - 0.5).abs() < 1e-12);
        // A shape larger than the image covers every pixel exactly.
        let big = covered(&[(-5.0, -5.0), (9.0, -4.0), (8.0, 9.0), (-6.0, 8.0)], 4, 4);
        assert!(big.data().iter().all(|&v| (v - 1.0).abs() < 1e-12));
    }

    #[test]
    fn holes_and_both_windings() {
        let outer = [(0.0, 0.0), (6.0, 0.0), (6.0, 6.0), (0.0, 6.0)];
        let hole = [(2.0, 2.0), (2.0, 4.0), (4.0, 4.0), (4.0, 2.0)]; // opposite direction
        let mut c = Coverage::new(6, 6);
        c.contour(&outer);
        c.contour(&hole);
        let p = c.finish();
        assert_eq!(p.get(2, 2), 0.0);
        assert_eq!(p.get(1, 1), 1.0);
        let reversed: Vec<_> = outer.iter().rev().copied().collect();
        assert_eq!(covered(&reversed, 6, 6), covered(&outer, 6, 6));
    }
}
