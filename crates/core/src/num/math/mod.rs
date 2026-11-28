//! Deterministic transcendental functions (docs/ALGORITHME.md §14.4).
//!
//! `std`'s `f64::exp`, `sin`, `powf`… call the platform's libm, whose last bit can differ between
//! native targets and WebAssembly. These are pure-Rust ports of the musl / FreeBSD msun (Sun
//! fdlibm) routines, taken from the Rust `libm` crate (MIT, see `THIRD_PARTY.md`): only IEEE
//! `+ - * /` and `sqrt`, no `fma`, no arch-specific paths, so they return the same bits
//! everywhere. Accuracy is about 1 ulp (the tests compare them with `std` on dense samples).
//!
//! FP-exception side effects of the C originals (inexact, underflow flags) are dropped; returned
//! values are unchanged.

// Ported numeric code: kept as close to the original as possible so it can be diffed against
// libm/musl, hence the bit-exact literals, terse names and C-style control flow.
#![allow(
    clippy::approx_constant,
    clippy::excessive_precision,
    clippy::unreadable_literal,
    clippy::similar_names,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::needless_range_loop,
    clippy::manual_midpoint,
    // `x - x` and `0.0 / (x - x)` deliberately build NaN (or propagate it), as in C.
    clippy::eq_op
)]

mod exp_log;
mod inv_trig;
mod misc;
mod pow;
mod rem_pio2_large;
mod support;
mod trig;

#[cfg(test)]
#[allow(clippy::disallowed_methods)] // compares against `std`'s transcendental functions
mod tests;

pub use exp_log::{exp, ln, ln_1p};
pub use inv_trig::{acos, asin, atan, atan2};
pub use misc::{cbrt, hypot};
pub use pow::pow;
pub use trig::{cos, sin, sin_cos, tan};
