/// Errors from codec encode/decode operations.
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
}

/// Errors from DICOS file operations.
#[derive(Debug, thiserror::Error)]
pub enum DicosError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid DICOS file: {0}")]
    InvalidFile(String),

    #[error("missing required attribute: ({group:#06x},{element:#06x})")]
    MissingAttribute { group: u16, element: u16 },

    #[error("invalid value for ({group:#06x},{element:#06x}): {reason}")]
    InvalidValue {
        group: u16,
        element: u16,
        reason: String,
    },

    #[error("unsupported transfer syntax: {0}")]
    UnsupportedTransferSyntax(String),

    #[error("codec error: {0}")]
    Codec(#[from] CodecError),

    #[error("validation error: {0}")]
    Validation(String),
}
