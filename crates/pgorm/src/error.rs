//! Error types for pgorm
//!
//! ## Error classification
//!
//! `OrmError` variants fall into two categories:
//!
//! **Recoverable** — the caller should match on these and handle them:
//! [`NotFound`](OrmError::NotFound), [`TooManyRows`](OrmError::TooManyRows),
//! [`UniqueViolation`](OrmError::UniqueViolation), [`ForeignKeyViolation`](OrmError::ForeignKeyViolation),
//! [`CheckViolation`](OrmError::CheckViolation), [`SerializationFailure`](OrmError::SerializationFailure),
//! [`DeadlockDetected`](OrmError::DeadlockDetected), [`StaleRecord`](OrmError::StaleRecord),
//! [`Timeout`](OrmError::Timeout), [`Validation`](OrmError::Validation).
//!
//! **Configuration / programming errors** — typically propagated with `?`:
//! [`Connection`](OrmError::Connection), [`Query`](OrmError::Query),
//! [`Decode`](OrmError::Decode), [`Serialization`](OrmError::Serialization),
//! [`Pool`](OrmError::Pool), [`Migration`](OrmError::Migration),
//! [`Other`](OrmError::Other).

use thiserror::Error;

/// Result type alias for pgorm operations
pub type OrmResult<T> = Result<T, OrmError>;

/// Error types for database operations.
///
/// Variants are grouped into **recoverable** (match and handle) and
/// **configuration/programming** errors (propagate with `?`).
/// Use [`is_recoverable`](Self::is_recoverable) to check programmatically.
#[derive(Debug, Error)]
pub enum OrmError {
    // ── Configuration / programming errors ──────────────────────────────────
    /// Database connection error (configuration or network).
    #[error("Connection error: {0}")]
    Connection(String),

    /// Query execution error (SQL syntax, runtime DB error).
    #[error("Query error: {0}")]
    Query(#[from] tokio_postgres::Error),

    /// Row decode/mapping error (schema drift or type mismatch).
    #[error("Decode error on column '{column}': {message}")]
    Decode { column: String, message: String },

    /// Serialization error (programming error).
    #[error("Serialization error: {0}")]
    Serialization(String),

    /// Pool error (exhaustion or misconfiguration).
    #[cfg(feature = "pool")]
    #[error("Pool error: {0}")]
    Pool(String),

    /// Migration error (feature: `migrate`).
    #[cfg(feature = "migrate")]
    #[error("Migration error: {0}")]
    Migration(String),

    /// Other / catch-all error.
    #[error("{0}")]
    Other(String),

    // ── Recoverable errors (match and handle) ───────────────────────────────
    /// Row not found (`fetch_one` returned no rows).
    #[error("Not found: {0}")]
    NotFound(String),

    /// Query returned more rows than expected.
    #[error("Too many rows: expected {expected}, got {got}")]
    TooManyRows { expected: usize, got: usize },

    /// Unique constraint violation (DB error code 23505).
    #[error("Unique constraint violation: {0}")]
    UniqueViolation(String),

    /// Foreign key constraint violation (DB error code 23503).
    #[error("Foreign key violation: {0}")]
    ForeignKeyViolation(String),

    /// Check constraint violation (DB error code 23514).
    #[error("Check constraint violation: {0}")]
    CheckViolation(String),

    /// Serialization failure (DB error code 40001).
    ///
    /// This occurs when a transaction cannot be committed due to a serialization
    /// conflict. The recommended response is to retry the transaction.
    #[error("Serialization failure: {0}")]
    SerializationFailure(String),

    /// Deadlock detected (DB error code 40P01).
    ///
    /// This occurs when two or more transactions are waiting on each other.
    /// The recommended response is to retry the transaction.
    #[error("Deadlock detected: {0}")]
    DeadlockDetected(String),

    /// Input validation error.
    #[error("Validation error: {0}")]
    Validation(String),

    /// Query timeout.
    #[error("Query timeout after {0:?}")]
    Timeout(std::time::Duration),

    /// Optimistic locking conflict: record was modified by another transaction.
    #[error("Stale record: {table} with id {id} (expected version {expected_version})")]
    StaleRecord {
        table: &'static str,
        id: String,
        expected_version: i64,
    },
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

    /// Create a too-many-rows error.
    pub fn too_many_rows(expected: usize, got: usize) -> Self {
        Self::TooManyRows { expected, got }
    }

    /// Create a validation error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::Validation(message.into())
    }

    /// Create a stale record error for optimistic locking conflicts
    pub fn stale_record(table: &'static str, id: impl ToString, expected_version: i64) -> Self {
        Self::StaleRecord {
            table,
            id: id.to_string(),
            expected_version,
        }
    }

    /// Returns `true` if this error is recoverable (the caller should handle it).
    ///
    /// Recoverable errors include: `NotFound`, `TooManyRows`, `UniqueViolation`,
    /// `ForeignKeyViolation`, `CheckViolation`, `SerializationFailure`,
    /// `DeadlockDetected`, `StaleRecord`, `Timeout`, `Validation`.
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::NotFound(_)
                | Self::TooManyRows { .. }
                | Self::UniqueViolation(_)
                | Self::ForeignKeyViolation(_)
                | Self::CheckViolation(_)
                | Self::SerializationFailure(_)
                | Self::DeadlockDetected(_)
                | Self::StaleRecord { .. }
                | Self::Timeout(_)
                | Self::Validation(_)
        )
    }

    /// Returns `true` if this error is retryable (transaction should be retried).
    ///
    /// Retryable errors include: `SerializationFailure`, `DeadlockDetected`.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::SerializationFailure(_) | Self::DeadlockDetected(_)
        )
    }

    /// Check if this is a unique violation error
    pub fn is_unique_violation(&self) -> bool {
        matches!(self, Self::UniqueViolation(_))
    }

    /// Check if this is a not found error
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }

    /// Check if this is a too-many-rows error.
    pub fn is_too_many_rows(&self) -> bool {
        matches!(self, Self::TooManyRows { .. })
    }

    /// Check if this is a timeout error
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout(_))
    }

    /// Check if this is a stale record (optimistic lock) error
    pub fn is_stale_record(&self) -> bool {
        matches!(self, Self::StaleRecord { .. })
    }

    /// Return the PostgreSQL SQLSTATE code if this error originated from the database.
    ///
    /// Returns `None` for non-database errors (e.g. `NotFound`, `Timeout`, `Validation`).
    ///
    /// # Example
    ///
    /// ```ignore
    /// match result {
    ///     Err(ref e) if e.sqlstate() == Some("23505") => { /* unique violation */ }
    ///     Err(ref e) if e.sqlstate() == Some("40001") => { /* retry transaction */ }
    ///     _ => {}
    /// }
    /// ```
    pub fn sqlstate(&self) -> Option<&str> {
        match self {
            Self::Query(e) => e.as_db_error().map(|db| db.code().code()),
            Self::UniqueViolation(_) => Some("23505"),
            Self::ForeignKeyViolation(_) => Some("23503"),
            Self::CheckViolation(_) => Some("23514"),
            Self::SerializationFailure(_) => Some("40001"),
            Self::DeadlockDetected(_) => Some("40P01"),
            _ => None,
        }
    }

    /// Parse a tokio_postgres error into a more specific OrmError
    pub fn from_db_error(err: tokio_postgres::Error) -> Self {
        if let Some(db_err) = err.as_db_error() {
            let constraint = db_err.constraint().unwrap_or("unknown");
            let message = db_err.message();

            match db_err.code().code() {
                "23505" => return Self::UniqueViolation(format!("{constraint}: {message}")),
                "23503" => {
                    return Self::ForeignKeyViolation(format!("{constraint}: {message}"));
                }
                "23514" => return Self::CheckViolation(format!("{constraint}: {message}")),
                "40001" => return Self::SerializationFailure(message.to_string()),
                "40P01" => return Self::DeadlockDetected(message.to_string()),
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

/// Emit a pgorm warning message.
///
/// Uses `tracing::warn!` when the `tracing` feature is enabled,
/// falls back to `eprintln!` otherwise.
pub(crate) fn pgorm_warn(msg: &str) {
    #[cfg(feature = "tracing")]
    tracing::warn!(target: "pgorm", "{}", msg);
    #[cfg(not(feature = "tracing"))]
    eprintln!("{msg}");
}
