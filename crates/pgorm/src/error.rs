//! Error types for pgorm

use thiserror::Error;

/// Result type alias for pgorm operations
pub type OrmResult<T> = Result<T, OrmError>;

/// Error types for database operations
#[derive(Debug, Error)]
pub enum OrmError {
    /// Database connection error
    #[error("Connection error: {0}")]
    Connection(String),

    /// Query execution error
    #[error("Query error: {0}")]
    Query(#[from] tokio_postgres::Error),

    /// Row not found
    #[error("Not found: {0}")]
    NotFound(String),

    /// Unique constraint violation
    #[error("Unique constraint violation: {0}")]
    UniqueViolation(String),

    /// Foreign key constraint violation
    #[error("Foreign key violation: {0}")]
    ForeignKeyViolation(String),

    /// Check constraint violation
    #[error("Check constraint violation: {0}")]
    CheckViolation(String),

    /// Row decode/mapping error
    #[error("Decode error on column '{column}': {message}")]
    Decode { column: String, message: String },

    /// Validation error
    #[error("Validation error: {0}")]
    Validation(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Pool error
    #[cfg(feature = "pool")]
    #[error("Pool error: {0}")]
    Pool(String),

    /// Query timeout error
    #[error("Query timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// Migration error
    #[cfg(feature = "migrate")]
    #[error("Migration error: {0}")]
    Migration(String),

    /// Other errors
    #[error("{0}")]
    Other(String),
}

impl OrmError {
    /// Create a decode error for a specific column
    pub fn decode(column: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Decode {
            column: column.into(),
            message: message.into(),
        }
    }

    /// Create a not found error
    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    /// Create a validation error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Check if this is a unique violation error
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::UniqueViolation(_))
    }

    /// Check if this is a not found error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    /// Check if this is a timeout error
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }

    /// Parse a tokio_postgres error into a more specific OrmError
    pub fn from_db_error(err: tokio_postgres::Error) -> Self {
        if let Some(db_err) = err.as_db_error() {
            let constraint = db_err.constraint().unwrap_or("unknown");
            let message = db_err.message();

            match db_err.code().code() {
                "23505" => return Self::UniqueViolation(format!("{}: {}", constraint, message)),
                "23503" => {
                    return Self::ForeignKeyViolation(format!("{}: {}", constraint, message));
                }
                "23514" => return Self::CheckViolation(format!("{}: {}", constraint, message)),
                _ => {}
            }
        }
        Self::Query(err)
    }
}

#[cfg(feature = "pool")]
impl From<deadpool_postgres::PoolError> for OrmError {
    fn from(err: deadpool_postgres::PoolError) -> Self {
        Self::Pool(err.to_string())
    }
}

#[cfg(feature = "migrate")]
impl From<refinery::Error> for OrmError {
    fn from(err: refinery::Error) -> Self {
        Self::Migration(err.to_string())
    }
}
