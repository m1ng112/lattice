//! Error types for the synthesis pipeline.

/// Errors that can occur during synthesis.
#[derive(Debug, thiserror::Error)]
pub enum SynthesisError {
    #[error("LLM API error: {0}")]
    ApiError(String),

    #[error("parse error: {0}")]
    ParseError(String),

    #[error("verification failed: {0}")]
    VerificationFailed(String),

    #[error("max attempts exceeded")]
    MaxAttemptsExceeded,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
