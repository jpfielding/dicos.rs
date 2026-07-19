/// Errors from JPEG 2000 codec encode/decode operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CodecError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("dimension mismatch: expected {expected} pixels, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}
