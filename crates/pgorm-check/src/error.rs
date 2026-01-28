//! Error types for pgorm-check

use std::fmt;

/// Result type for pgorm-check operations.
pub type CheckResult<T> = Result<T, CheckError>;

/// Error type for pgorm-check operations.
#[derive(Debug)]
pub enum CheckError {
    /// Database error from tokio-postgres.
    Database(tokio_postgres::Error),
    /// Validation error (e.g., SQL parse error, missing schema).
    Validation(String),
    /// Serialization/deserialization error.
    Serialization(String),
    /// Decode error when reading a column.
    Decode { column: String, message: String },
    /// IO or other error.
    Other(String),
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Database(e) => write!(f, "Database error: {}", e),
            CheckError::Validation(msg) => write!(f, "Validation error: {}", msg),
            CheckError::Serialization(msg) => write!(f, "Serialization error: {}", msg),
            CheckError::Decode { column, message } => {
                write!(f, "Decode error for column '{}': {}", column, message)
            }
            CheckError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Database(e) => Some(e),
            _ => None,
        }
    }
}

impl From<tokio_postgres::Error> for CheckError {
    fn from(e: tokio_postgres::Error) -> Self {
        CheckError::Database(e)
    }
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
