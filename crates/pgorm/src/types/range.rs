//! PostgreSQL Range type support.
//!
//! Provides [`Range<T>`] and [`Bound<T>`] types that map to PostgreSQL range types
//! (`int4range`, `int8range`, `tsrange`, `tstzrange`, `daterange`, `numrange`).

use crate::row::PgType;
use bytes::BytesMut;
use std::error::Error;
use tokio_postgres::types::{FromSql, IsNull, ToSql, Type};

// PostgreSQL range flags (binary protocol)
const RANGE_EMPTY: u8 = 0x01;
const RANGE_HAS_LOWER: u8 = 0x02;
const RANGE_HAS_UPPER: u8 = 0x04;
const RANGE_LOWER_INCLUSIVE: u8 = 0x08;
const RANGE_UPPER_INCLUSIVE: u8 = 0x10;

/// A bound of a range (inclusive or exclusive).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Bound<T> {
    /// The bound includes the value: `[value` or `value]`
    Inclusive(T),
    /// The bound excludes the value: `(value` or `value)`
    Exclusive(T),
}

impl<T> Bound<T> {
    /// Returns a reference to the inner value.
    pub fn value(&self) -> &T {
        match self {
            Bound::Inclusive(v) | Bound::Exclusive(v) => v,
        }
    }

    /// Returns `true` if this bound is inclusive.
    pub fn is_inclusive(&self) -> bool {
        matches!(self, Bound::Inclusive(_))
    }

    /// Consumes the bound and returns the inner value.
    pub fn into_value(self) -> T {
        match self {
            Bound::Inclusive(v) | Bound::Exclusive(v) => v,
        }
    }
}

/// A PostgreSQL range type.
///
/// Represents a range of values with optional lower and upper bounds.
/// `None` bounds represent unbounded (infinite) endpoints.
///
/// # Examples
///
/// ```ignore
/// use pgorm::types::{Range, Bound};
///
/// // [1, 10] — closed range
/// let r = Range::<i32>::inclusive(1, 10);
///
/// // [1, 10) — half-open range (most common for integers)
/// let r = Range::<i32>::lower_inc(1, 10);
///
/// // Empty range
/// let r = Range::<i32>::empty();
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Range<T> {
    /// Lower bound, or `None` for unbounded.
    pub lower: Option<Bound<T>>,
    /// Upper bound, or `None` for unbounded.
    pub upper: Option<Bound<T>>,
    /// Whether this is the empty range.
    empty: bool,
}

impl<T> Range<T> {
    /// Creates a new range with the given bounds.
    pub fn new(lower: Option<Bound<T>>, upper: Option<Bound<T>>) -> Self {
        Self {
            lower,
            upper,
            empty: false,
        }
    }

    /// Creates an empty range (contains no values).
    pub fn empty() -> Self {
        Self {
            lower: None,
            upper: None,
            empty: true,
        }
    }

    /// Creates a closed range `[lower, upper]`.
    pub fn inclusive(lower: T, upper: T) -> Self {
        Self::new(
            Some(Bound::Inclusive(lower)),
            Some(Bound::Inclusive(upper)),
        )
    }

    /// Creates an open range `(lower, upper)`.
    pub fn exclusive(lower: T, upper: T) -> Self {
        Self::new(
            Some(Bound::Exclusive(lower)),
            Some(Bound::Exclusive(upper)),
        )
    }

    /// Creates a half-open range `[lower, upper)`.
    pub fn lower_inc(lower: T, upper: T) -> Self {
        Self::new(
            Some(Bound::Inclusive(lower)),
            Some(Bound::Exclusive(upper)),
        )
    }

    /// Creates a half-open range `(lower, upper]`.
    pub fn upper_inc(lower: T, upper: T) -> Self {
        Self::new(
            Some(Bound::Exclusive(lower)),
            Some(Bound::Inclusive(upper)),
        )
    }

    /// Returns `true` if this is the empty range.
    pub fn is_empty(&self) -> bool {
        self.empty
    }

    /// Creates an unbounded range `(-infinity, +infinity)`.
    pub fn unbounded() -> Self {
        Self::new(None, None)
    }
}

// ─── ToSql / FromSql ────────────────────────────────────────────────────────

/// Helper: resolve the element type from a range type.
fn range_element_type(range_ty: &Type) -> Option<Type> {
    // PostgreSQL range types have a well-known mapping to element types.
    match *range_ty {
        Type::INT4_RANGE => Some(Type::INT4),
        Type::INT8_RANGE => Some(Type::INT8),
        Type::NUM_RANGE => Some(Type::NUMERIC),
        Type::TS_RANGE => Some(Type::TIMESTAMP),
        Type::TSTZ_RANGE => Some(Type::TIMESTAMPTZ),
        Type::DATE_RANGE => Some(Type::DATE),
        _ => None,
    }
}

impl<T> ToSql for Range<T>
where
    T: ToSql + 'static,
{
    fn to_sql(
        &self,
        ty: &Type,
        out: &mut BytesMut,
    ) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let element_type = range_element_type(ty)
            .ok_or_else(|| format!("unsupported range type: {}", ty))?;

        if self.empty {
            out.extend_from_slice(&[RANGE_EMPTY]);
            return Ok(IsNull::No);
        }

        let mut flags: u8 = 0;
        if self.lower.is_some() {
            flags |= RANGE_HAS_LOWER;
        }
        if self.upper.is_some() {
            flags |= RANGE_HAS_UPPER;
        }
        if let Some(Bound::Inclusive(_)) = &self.lower {
            flags |= RANGE_LOWER_INCLUSIVE;
        }
        if let Some(Bound::Inclusive(_)) = &self.upper {
            flags |= RANGE_UPPER_INCLUSIVE;
        }

        out.extend_from_slice(&[flags]);

        if let Some(bound) = &self.lower {
            encode_bound(bound.value(), &element_type, out)?;
        }
        if let Some(bound) = &self.upper {
            encode_bound(bound.value(), &element_type, out)?;
        }

        Ok(IsNull::No)
    }

    fn accepts(ty: &Type) -> bool {
        matches!(
            *ty,
            Type::INT4_RANGE
                | Type::INT8_RANGE
                | Type::NUM_RANGE
                | Type::TS_RANGE
                | Type::TSTZ_RANGE
                | Type::DATE_RANGE
        )
    }

    tokio_postgres::types::to_sql_checked!();
}

fn encode_bound<T: ToSql>(
    value: &T,
    element_type: &Type,
    out: &mut BytesMut,
) -> Result<(), Box<dyn Error + Sync + Send>> {
    // Reserve space for the 4-byte length prefix
    let len_pos = out.len();
    out.extend_from_slice(&[0u8; 4]);

    let start = out.len();
    match value.to_sql(element_type, out)? {
        IsNull::Yes => {
            // NULL bound — should not happen in well-formed ranges, but handle it.
            let len_bytes = (-1_i32).to_be_bytes();
            out[len_pos..len_pos + 4].copy_from_slice(&len_bytes);
        }
        IsNull::No => {
            let written = (out.len() - start) as i32;
            let len_bytes = written.to_be_bytes();
            out[len_pos..len_pos + 4].copy_from_slice(&len_bytes);
        }
    }
    Ok(())
}

impl<'a, T> FromSql<'a> for Range<T>
where
    T: FromSql<'a> + 'static,
{
    fn from_sql(
        ty: &Type,
        raw: &'a [u8],
    ) -> Result<Self, Box<dyn Error + Sync + Send>> {
        if raw.is_empty() {
            return Err("empty range data".into());
        }

        let element_type = range_element_type(ty)
            .ok_or_else(|| format!("unsupported range type: {}", ty))?;

        let flags = raw[0];
        let mut pos = 1;

        if flags & RANGE_EMPTY != 0 {
            return Ok(Range::empty());
        }

        let lower = if flags & RANGE_HAS_LOWER != 0 {
            let (value, new_pos) = decode_bound::<T>(&element_type, raw, pos)?;
            pos = new_pos;
            let bound = if flags & RANGE_LOWER_INCLUSIVE != 0 {
                Bound::Inclusive(value)
            } else {
                Bound::Exclusive(value)
            };
            Some(bound)
        } else {
            None
        };

        let upper = if flags & RANGE_HAS_UPPER != 0 {
            let (value, _new_pos) = decode_bound::<T>(&element_type, raw, pos)?;
            let bound = if flags & RANGE_UPPER_INCLUSIVE != 0 {
                Bound::Inclusive(value)
            } else {
                Bound::Exclusive(value)
            };
            Some(bound)
        } else {
            None
        };

        Ok(Range {
            lower,
            upper,
            empty: false,
        })
    }

    fn accepts(ty: &Type) -> bool {
        matches!(
            *ty,
            Type::INT4_RANGE
                | Type::INT8_RANGE
                | Type::NUM_RANGE
                | Type::TS_RANGE
                | Type::TSTZ_RANGE
                | Type::DATE_RANGE
        )
    }
}

fn decode_bound<'a, T: FromSql<'a>>(
    element_type: &Type,
    raw: &'a [u8],
    pos: usize,
) -> Result<(T, usize), Box<dyn Error + Sync + Send>> {
    if pos + 4 > raw.len() {
        return Err("range bound: insufficient data for length".into());
    }
    let len = i32::from_be_bytes(raw[pos..pos + 4].try_into().unwrap());
    let pos = pos + 4;

    if len < 0 {
        return Err("NULL range bound is not supported".into());
    }
    let len = len as usize;
    if pos + len > raw.len() {
        return Err("range bound: insufficient data for value".into());
    }

    let value = T::from_sql(element_type, &raw[pos..pos + len])?;
    Ok((value, pos + len))
}

// ─── PgType implementations ─────────────────────────────────────────────────

impl PgType for Range<i32> {
    fn pg_array_type() -> &'static str {
        "int4range[]"
    }
}

impl PgType for Range<i64> {
    fn pg_array_type() -> &'static str {
        "int8range[]"
    }
}

impl PgType for Range<chrono::NaiveDateTime> {
    fn pg_array_type() -> &'static str {
        "tsrange[]"
    }
}

impl<Tz: chrono::TimeZone> PgType for Range<chrono::DateTime<Tz>> {
    fn pg_array_type() -> &'static str {
        "tstzrange[]"
    }
}

impl PgType for Range<chrono::NaiveDate> {
    fn pg_array_type() -> &'static str {
        "daterange[]"
    }
}

#[cfg(feature = "rust_decimal")]
impl PgType for Range<rust_decimal::Decimal> {
    fn pg_array_type() -> &'static str {
        "numrange[]"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_constructors() {
        let r = Range::<i32>::inclusive(1, 10);
        assert!(!r.is_empty());
        assert_eq!(r.lower, Some(Bound::Inclusive(1)));
        assert_eq!(r.upper, Some(Bound::Inclusive(10)));

        let r = Range::<i32>::exclusive(1, 10);
        assert_eq!(r.lower, Some(Bound::Exclusive(1)));
        assert_eq!(r.upper, Some(Bound::Exclusive(10)));

        let r = Range::<i32>::lower_inc(1, 10);
        assert_eq!(r.lower, Some(Bound::Inclusive(1)));
        assert_eq!(r.upper, Some(Bound::Exclusive(10)));

        let r = Range::<i32>::upper_inc(1, 10);
        assert_eq!(r.lower, Some(Bound::Exclusive(1)));
        assert_eq!(r.upper, Some(Bound::Inclusive(10)));

        let r = Range::<i32>::empty();
        assert!(r.is_empty());

        let r = Range::<i32>::unbounded();
        assert!(!r.is_empty());
        assert_eq!(r.lower, None);
        assert_eq!(r.upper, None);
    }

    #[test]
    fn bound_helpers() {
        let b = Bound::Inclusive(42);
        assert_eq!(b.value(), &42);
        assert!(b.is_inclusive());
        assert_eq!(b.into_value(), 42);

        let b = Bound::Exclusive(42);
        assert!(!b.is_inclusive());
    }

    #[test]
    fn pg_type_range_i32() {
        assert_eq!(<Range<i32> as PgType>::pg_array_type(), "int4range[]");
    }

    #[test]
    fn pg_type_range_i64() {
        assert_eq!(<Range<i64> as PgType>::pg_array_type(), "int8range[]");
    }

    #[test]
    fn pg_type_range_naive_date() {
        assert_eq!(
            <Range<chrono::NaiveDate> as PgType>::pg_array_type(),
            "daterange[]"
        );
    }

    #[test]
    fn pg_type_range_naive_datetime() {
        assert_eq!(
            <Range<chrono::NaiveDateTime> as PgType>::pg_array_type(),
            "tsrange[]"
        );
    }

    #[test]
    fn pg_type_range_datetime_utc() {
        assert_eq!(
            <Range<chrono::DateTime<chrono::Utc>> as PgType>::pg_array_type(),
            "tstzrange[]"
        );
    }

    #[cfg(feature = "rust_decimal")]
    #[test]
    fn pg_type_range_decimal() {
        assert_eq!(
            <Range<rust_decimal::Decimal> as PgType>::pg_array_type(),
            "numrange[]"
        );
    }

    #[test]
    fn pg_type_option_range() {
        assert_eq!(
            <Option<Range<i32>> as PgType>::pg_array_type(),
            "int4range[]"
        );
    }

    #[test]
    fn range_binary_roundtrip_empty() {
        let range = Range::<i32>::empty();
        let ty = Type::INT4_RANGE;
        let mut buf = BytesMut::new();
        range.to_sql(&ty, &mut buf).unwrap();

        let decoded = Range::<i32>::from_sql(&ty, &buf).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn range_binary_roundtrip_inclusive() {
        let range = Range::<i32>::inclusive(1, 10);
        let ty = Type::INT4_RANGE;
        let mut buf = BytesMut::new();
        range.to_sql(&ty, &mut buf).unwrap();

        let decoded = Range::<i32>::from_sql(&ty, &buf).unwrap();
        assert_eq!(decoded.lower, Some(Bound::Inclusive(1)));
        assert_eq!(decoded.upper, Some(Bound::Inclusive(10)));
    }

    #[test]
    fn range_binary_roundtrip_lower_inc() {
        let range = Range::<i64>::lower_inc(100, 200);
        let ty = Type::INT8_RANGE;
        let mut buf = BytesMut::new();
        range.to_sql(&ty, &mut buf).unwrap();

        let decoded = Range::<i64>::from_sql(&ty, &buf).unwrap();
        assert_eq!(decoded.lower, Some(Bound::Inclusive(100)));
        assert_eq!(decoded.upper, Some(Bound::Exclusive(200)));
    }

    #[test]
    fn range_binary_roundtrip_unbounded_lower() {
        let range = Range::<i32>::new(None, Some(Bound::Exclusive(10)));
        let ty = Type::INT4_RANGE;
        let mut buf = BytesMut::new();
        range.to_sql(&ty, &mut buf).unwrap();

        let decoded = Range::<i32>::from_sql(&ty, &buf).unwrap();
        assert_eq!(decoded.lower, None);
        assert_eq!(decoded.upper, Some(Bound::Exclusive(10)));
    }
}
