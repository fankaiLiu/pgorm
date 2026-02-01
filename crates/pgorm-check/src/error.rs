//! Error types for pgorm-check

use thiserror::Error;

/// Result type for pgorm-check operations.
pub type CheckResult<T> = Result<T, CheckError>;

/// Error type for pgorm-check operations.
#[derive(Debug, Error)]
pub enum CheckError {
    /// Database error from tokio-postgres.
    #[error("Database error: {0}")]
    Database(#[from] tokio_postgres::Error),
    /// Validation error (e.g., SQL parse error, missing schema).
    #[error("Validation error: {0}")]
    Validation(String),
    /// Serialization/deserialization error.
    #[error("Serialization error: {0}")]
    Serialization(String),
    /// Decode error when reading a column.
    #[error("Decode error for column '{column}': {message}")]
    Decode { column: String, message: String },
    /// IO or other error.
    #[error("{0}")]
    Other(String),
}

impl CheckError {
    /// Create a decode error.
    pub fn decode(column: impl Into<String>, message: impl Into<String>) -> Self {
        CheckError::Decode {
            column: column.into(),
            message: message.into(),
        }
    }
}
