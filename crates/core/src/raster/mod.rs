//! Raster building blocks (docs/ALGORITHME.md §14.1): image planes and the exact-coverage
//! rasterizer used to render ground-truth scenes and, later, to verify the tracing.

pub mod coverage;
pub mod plane;

pub use coverage::Coverage;
pub use plane::Plane;
