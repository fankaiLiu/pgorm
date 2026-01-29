//! Row mapping traits and utilities

use crate::error::OrmResult;
use tokio_postgres::Row;

/// Trait that maps Rust types to PostgreSQL type names for UNNEST casts.
///
/// This is used by derive macros to generate correct type casts in batch operations.
pub trait PgType {
    /// Returns the PostgreSQL array type name (e.g., "text[]", "bigint[]")
    fn pg_array_type() -> &'static str;
}

// Primitive type implementations
impl PgType for i16 {
    fn pg_array_type() -> &'static str {
        "smallint[]"
    }
}

impl PgType for i32 {
    fn pg_array_type() -> &'static str {
        "integer[]"
    }
}

impl PgType for i64 {
    fn pg_array_type() -> &'static str {
        "bigint[]"
    }
}

impl PgType for f32 {
    fn pg_array_type() -> &'static str {
        "real[]"
    }
}

impl PgType for f64 {
    fn pg_array_type() -> &'static str {
        "double precision[]"
    }
}

impl PgType for bool {
    fn pg_array_type() -> &'static str {
        "boolean[]"
    }
}

impl PgType for String {
    fn pg_array_type() -> &'static str {
        "text[]"
    }
}

impl PgType for &str {
    fn pg_array_type() -> &'static str {
        "text[]"
    }
}

impl PgType for Vec<u8> {
    fn pg_array_type() -> &'static str {
        "bytea[]"
    }
}

impl PgType for serde_json::Value {
    fn pg_array_type() -> &'static str {
        "jsonb[]"
    }
}

impl<'a> PgType for &'a serde_json::Value {
    fn pg_array_type() -> &'static str {
        "jsonb[]"
    }
}

impl<T> PgType for tokio_postgres::types::Json<T> {
    fn pg_array_type() -> &'static str {
        "jsonb[]"
    }
}

impl PgType for uuid::Uuid {
    fn pg_array_type() -> &'static str {
        "uuid[]"
    }
}

impl PgType for chrono::NaiveDate {
    fn pg_array_type() -> &'static str {
        "date[]"
    }
}

impl PgType for chrono::NaiveTime {
    fn pg_array_type() -> &'static str {
        "time[]"
    }
}

impl PgType for chrono::NaiveDateTime {
    fn pg_array_type() -> &'static str {
        "timestamp[]"
    }
}

impl<Tz: chrono::TimeZone> PgType for chrono::DateTime<Tz> {
    fn pg_array_type() -> &'static str {
        "timestamptz[]"
    }
}

// Option<T> delegates to inner type
impl<T: PgType> PgType for Option<T> {
    fn pg_array_type() -> &'static str {
        T::pg_array_type()
    }
}

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

#[cfg(test)]
mod tests {
    use super::PgType;

    #[test]
    fn pg_type_jsonb_value() {
        assert_eq!(<serde_json::Value as PgType>::pg_array_type(), "jsonb[]");
    }

    #[test]
    fn pg_type_jsonb_value_ref() {
        assert_eq!(<&serde_json::Value as PgType>::pg_array_type(), "jsonb[]");
    }

    #[test]
    fn pg_type_jsonb_json_wrapper() {
        assert_eq!(
            <tokio_postgres::types::Json<serde_json::Value> as PgType>::pg_array_type(),
            "jsonb[]"
        );
    }
}
