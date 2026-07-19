/// Errors from JPEG-LS codec encode/decode operations.
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

    /// A JPEG marker (0xFFxx, xx >= 0x80) was encountered while decoding the
    /// entropy-coded scan. Carries the full marker code so the caller can
    /// distinguish EOI from LSE/RSTn etc.
    // TODO(t87): wired into the decoder in L3/L4; produced today only by the
    // T.87 `BitReader` (unit-tested).
    #[allow(dead_code)]
    #[error("marker 0x{0:04X} encountered in entropy-coded scan")]
    Marker(u16),
}
