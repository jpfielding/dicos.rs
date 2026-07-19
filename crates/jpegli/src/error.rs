/// Errors from JPEG Lossless codec encode/decode operations.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid data: {0}")]
    InvalidData(String),

    #[error("unsupported: {0}")]
    Unsupported(String),

    #[error("dimension mismatch: expected {expected} pixels, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    /// The entropy-coded scan or a marker segment ended before all expected
    /// data was read. `offset` is the byte position within the input where
    /// decoding stopped; `context` names the phase.
    #[error("truncated JPEG Lossless stream at offset {offset} ({context})")]
    Truncated {
        offset: usize,
        context: &'static str,
    },

    /// A caller-supplied or in-stream parameter was outside the permitted
    /// range.
    #[error("invalid parameter {name}={value} (allowed: {allowed})")]
    InvalidParameter {
        name: &'static str,
        value: i64,
        allowed: &'static str,
    },
}
