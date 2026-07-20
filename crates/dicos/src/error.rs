/// Errors from codec encode/decode operations.
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

    /// A codec backend failed. Carries the backend name and its underlying error.
    ///
    /// Used by the codec registry to attribute failures to a specific backend
    /// while preserving the original error via [`std::error::Error::source`].
    #[error("{codec} codec error: {source}")]
    Backend {
        /// The codec backend name (e.g. `"jpegls"`).
        codec: &'static str,
        /// The underlying backend error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync + 'static>,
    },
}

/// Errors from DICOS file operations.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DicosError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Last-resort, free-form file error.
    ///
    /// Reader paths must prefer the typed variants below (`BadPreamble`,
    /// `NestingTooDeep`, `UnexpectedTag`, `LengthExceedsLimit`,
    /// `LengthExceedsBuffer`, `UndefinedLengthInFixedSq`, `Truncated`). This
    /// variant is retained for external construction (e.g. `roxel`) and for
    /// cases that do not map onto a typed variant.
    #[error("invalid DICOS file: {0}")]
    InvalidFile(String),

    /// The file is not a valid DICOM/DICOS Part-10 stream.
    #[error("not a DICOM/DICOS Part-10 file: {reason}")]
    BadPreamble {
        /// Why the preamble/magic was rejected.
        reason: &'static str,
    },

    /// Sequence/item nesting exceeded the maximum allowed depth.
    #[error("sequence nesting exceeds maximum depth ({max})")]
    NestingTooDeep {
        /// The maximum nesting depth that was exceeded.
        max: usize,
    },

    /// A structural tag did not match what the parser expected at this point.
    #[error("{context}: expected tag {expected}, got {got}")]
    UnexpectedTag {
        /// The tag the parser required.
        expected: crate::tag::Tag,
        /// The tag actually read.
        got: crate::tag::Tag,
        /// Where in the parse this occurred.
        context: &'static str,
    },

    /// A declared element length exceeded the configured allocation limit.
    #[error("element length {length} exceeds limit ({limit})")]
    LengthExceedsLimit {
        /// The declared length.
        length: usize,
        /// The configured per-element ceiling.
        limit: usize,
    },

    /// A declared length exceeded the bytes remaining in the enclosing buffer.
    #[error("length {length} exceeds remaining buffer ({remaining})")]
    LengthExceedsBuffer {
        /// The declared length.
        length: usize,
        /// The bytes actually remaining.
        remaining: usize,
    },

    /// An undefined-length item appeared inside a fixed-length sequence.
    #[error("undefined-length item inside fixed-length SQ")]
    UndefinedLengthInFixedSq,

    /// The stream ended in the middle of an element or header.
    #[error("truncated at byte {offset} ({context})")]
    Truncated {
        /// The byte offset at which the truncation was detected.
        offset: u64,
        /// What was being read when the stream ended.
        context: &'static str,
    },

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
