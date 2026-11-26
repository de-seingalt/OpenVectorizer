/// Errors returned by the engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid image: {0}")]
    InvalidImage(String),
    #[error("invalid option: {0}")]
    InvalidOption(String),
    #[error("tracing failed: {0}")]
    Trace(String),
}
