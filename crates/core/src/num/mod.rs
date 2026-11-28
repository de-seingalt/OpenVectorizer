//! Numerical toolbox shared by every stage (docs/ALGORITHME.md §14.1).
//!
//! Everything here is deterministic: the same inputs give the same bits natively and in
//! WebAssembly. `math` replaces the transcendental functions of `std`, whose results may
//! differ in the last bit between platforms (§14.4).

pub mod math;
pub mod order;
pub mod rng;

use std::ops::Mul;

/// x², written as a product (`powi` may be lowered differently on each platform).
#[inline]
pub fn sq<T: Mul<Output = T> + Copy>(x: T) -> T {
    x * x
}

/// x³, written as a product.
#[inline]
pub fn cube<T: Mul<Output = T> + Copy>(x: T) -> T {
    x * x * x
}
