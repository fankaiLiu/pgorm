//! Row mapping traits and utilities

use crate::error::OrmResult;
use tokio_postgres::Row;

/// Trait for converting a database row into a Rust struct.
///
/// This trait should typically be derived using `#[derive(FromRow)]`
/// from the `pgorm-derive` crate.
///
/// # Example
///
/// ```ignore
/// use pgorm::FromRow;
///
/// #[derive(FromRow)]
/// struct User {
///     id: i64,
///     username: String,
///     email: Option<String>,
/// }
/// ```
pub trait FromRow: Sized {
    /// Convert a database row into Self
    fn from_row(row: &Row) -> OrmResult<Self>;
}

/// Extension trait for Row to provide typed access
pub trait RowExt {
    /// Try to get a column value, returning OrmError::Decode on failure
    fn try_get_column<T>(&self, column: &str) -> OrmResult<T>
    where
        T: for<'a> tokio_postgres::types::FromSql<'a>;
}

impl RowExt for Row {
    fn try_get_column<T>(&self, column: &str) -> OrmResult<T>
    where
        T: for<'a> tokio_postgres::types::FromSql<'a>,
    {
        self.try_get(column)
            .map_err(|e| crate::error::OrmError::decode(column, e.to_string()))
    }
}
