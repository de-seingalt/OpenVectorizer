//! Raster building blocks (docs/ALGORITHME.md §14.1): image planes, colour conversion, noise
//! model, input classification, Gaussian filters, pyramid, and the exact-coverage rasterizer.

pub mod classify;
pub mod coverage;
pub mod gauss;
pub mod noise;
pub mod oklab;
pub mod plane;
pub mod pyramid;

pub use coverage::Coverage;
pub use plane::Plane;

#[cfg(test)]
mod tests;
