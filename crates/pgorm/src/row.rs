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

impl PgType for &serde_json::Value {
    fn pg_array_type() -> &'static str {
        "jsonb[]"
    }
}

impl<T> PgType for tokio_postgres::types::Json<T> {
    fn pg_array_type() -> &'static str {
        "jsonb[]"
    }
}

impl PgType for std::net::IpAddr {
    fn pg_array_type() -> &'static str {
        "inet[]"
    }
}

impl PgType for std::net::Ipv4Addr {
    fn pg_array_type() -> &'static str {
        "inet[]"
    }
}

impl PgType for std::net::Ipv6Addr {
    fn pg_array_type() -> &'static str {
        "inet[]"
    }
}

impl PgType for uuid::Uuid {
    fn pg_array_type() -> &'static str {
        "uuid[]"
    }
}

#[cfg(feature = "rust_decimal")]
impl PgType for rust_decimal::Decimal {
    fn pg_array_type() -> &'static str {
        "numeric[]"
    }
}

#[cfg(feature = "rust_decimal")]
impl PgType for &rust_decimal::Decimal {
    fn pg_array_type() -> &'static str {
        "numeric[]"
    }
}

#[cfg(feature = "time")]
impl PgType for time::Date {
    fn pg_array_type() -> &'static str {
        "date[]"
    }
}

#[cfg(feature = "time")]
impl PgType for time::Time {
    fn pg_array_type() -> &'static str {
        "time[]"
    }
}

#[cfg(feature = "time")]
impl PgType for time::PrimitiveDateTime {
    fn pg_array_type() -> &'static str {
        "timestamp[]"
    }
}

#[cfg(feature = "time")]
impl PgType for time::OffsetDateTime {
    fn pg_array_type() -> &'static str {
        "timestamptz[]"
    }
}

#[cfg(feature = "cidr")]
impl PgType for cidr::IpCidr {
    fn pg_array_type() -> &'static str {
        "cidr[]"
    }
}

#[cfg(feature = "cidr")]
impl PgType for cidr::IpInet {
    fn pg_array_type() -> &'static str {
        "inet[]"
    }
}

#[cfg(feature = "geo_types")]
impl PgType for geo_types::Point<f64> {
    fn pg_array_type() -> &'static str {
        "point[]"
    }
}

#[cfg(feature = "geo_types")]
impl PgType for geo_types::Rect<f64> {
    fn pg_array_type() -> &'static str {
        "box[]"
    }
}

#[cfg(feature = "geo_types")]
impl PgType for geo_types::LineString<f64> {
    fn pg_array_type() -> &'static str {
        "path[]"
    }
}

#[cfg(feature = "eui48")]
impl PgType for eui48::MacAddress {
    fn pg_array_type() -> &'static str {
        "macaddr[]"
    }
}

#[cfg(feature = "bit_vec")]
impl PgType for bit_vec::BitVec {
    fn pg_array_type() -> &'static str {
        "varbit[]"
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

// Vec<T> represents a Postgres array column type (e.g. `text[]` -> `Vec<String>`).
// In UNNEST bulk insert, we bind a *list of rows* (Vec<...>), so we need one extra
// array dimension for the cast (e.g. `Vec<String>` -> `text[][]`).
impl<T: PgType> PgType for Vec<T> {
    fn pg_array_type() -> &'static str {
        use std::collections::HashMap;
        use std::sync::{Mutex, OnceLock};

        static CACHE: OnceLock<Mutex<HashMap<&'static str, &'static str>>> = OnceLock::new();
        let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));

        // `type_name` is `'static` and works even for non-`'static` reference types.
        let key = std::any::type_name::<Vec<T>>();

        // Fast path: already cached.
        if let Some(cached) = cache.lock().unwrap().get(key).copied() {
            return cached;
        }

        // Don't hold the lock while calling into `T::pg_array_type()` (it may recurse into this impl).
        let computed = format!("{}[]", T::pg_array_type());
        let computed: &'static str = Box::leak(computed.into_boxed_str());

        // Store (best-effort); if another thread won the race, return the stored value.
        let mut map = cache.lock().unwrap();
        map.entry(key).or_insert(computed)
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

    #[test]
    fn pg_type_inet_ipaddr() {
        assert_eq!(<std::net::IpAddr as PgType>::pg_array_type(), "inet[]");
    }

    #[test]
    fn pg_type_inet_ipv4addr() {
        assert_eq!(<std::net::Ipv4Addr as PgType>::pg_array_type(), "inet[]");
    }

    #[test]
    fn pg_type_inet_ipv6addr() {
        assert_eq!(<std::net::Ipv6Addr as PgType>::pg_array_type(), "inet[]");
    }

    #[cfg(feature = "rust_decimal")]
    #[test]
    fn pg_type_rust_decimal_decimal() {
        assert_eq!(
            <rust_decimal::Decimal as PgType>::pg_array_type(),
            "numeric[]"
        );
    }

    #[cfg(feature = "time")]
    #[test]
    fn pg_type_time_offset_datetime() {
        assert_eq!(
            <time::OffsetDateTime as PgType>::pg_array_type(),
            "timestamptz[]"
        );
    }

    #[cfg(feature = "cidr")]
    #[test]
    fn pg_type_cidr_ip_cidr() {
        assert_eq!(<cidr::IpCidr as PgType>::pg_array_type(), "cidr[]");
    }

    #[cfg(feature = "geo_types")]
    #[test]
    fn pg_type_geo_types_point() {
        assert_eq!(
            <geo_types::Point<f64> as PgType>::pg_array_type(),
            "point[]"
        );
    }

    #[cfg(feature = "eui48")]
    #[test]
    fn pg_type_eui48_mac_address() {
        assert_eq!(<eui48::MacAddress as PgType>::pg_array_type(), "macaddr[]");
    }

    #[cfg(feature = "bit_vec")]
    #[test]
    fn pg_type_bit_vec_bit_vec() {
        assert_eq!(<bit_vec::BitVec as PgType>::pg_array_type(), "varbit[]");
    }

    #[test]
    fn pg_type_vec_string_is_2d_text_array() {
        assert_eq!(<Vec<String> as PgType>::pg_array_type(), "text[][]");
    }

    #[test]
    fn pg_type_option_vec_string_is_2d_text_array() {
        assert_eq!(<Option<Vec<String>> as PgType>::pg_array_type(), "text[][]");
    }

    #[test]
    fn pg_type_vec_vec_string_is_3d_text_array() {
        assert_eq!(<Vec<Vec<String>> as PgType>::pg_array_type(), "text[][][]");
    }
}
