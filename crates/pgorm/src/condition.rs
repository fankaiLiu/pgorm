//! Query condition types for dynamic queries.
//!
//! This module provides [`Op`] (operator) and [`Condition`] primitives for building
//! flexible WHERE clauses with various comparison operators.

use crate::Ident;
use crate::error::OrmResult;
use crate::ident::IntoIdent;
use crate::sql::Sql;
use std::sync::Arc;
use tokio_postgres::types::ToSql;

/// Query operator for building conditions.
///
/// # Example
/// ```ignore
/// use pgorm::Op;
///
/// // Equality
/// Op::eq("value")
/// Op::ne("value")
///
/// // Comparison
/// Op::gt(100)
/// Op::gte(100)
/// Op::lt(100)
/// Op::lte(100)
///
/// // Pattern matching
/// Op::like("%pattern%")
/// Op::ilike("%pattern%")  // case-insensitive
/// Op::not_like("%pattern%")
///
/// // NULL checks
/// Op::<i32>::is_null()
/// Op::<i32>::is_not_null()
///
/// // List operations
/// Op::in_list(vec![1, 2, 3])
/// Op::not_in(vec![1, 2, 3])
///
/// // Range
/// Op::between(10, 20)
/// ```
#[derive(Debug, Clone)]
pub enum Op<T> {
    /// Equal: column = value
    Eq(T),
    /// Not equal: column != value
    Ne(T),
    /// Greater than: column > value
    Gt(T),
    /// Greater than or equal: column >= value
    Gte(T),
    /// Less than: column < value
    Lt(T),
    /// Less than or equal: column <= value
    Lte(T),
    /// LIKE pattern match
    Like(T),
    /// Case-insensitive LIKE (PostgreSQL ILIKE)
    Ilike(T),
    /// NOT LIKE pattern match
    NotLike(T),
    /// NOT ILIKE pattern match
    NotIlike(T),
    /// IS NULL
    IsNull,
    /// IS NOT NULL
    IsNotNull,
    /// IN (list)
    In(Vec<T>),
    /// NOT IN (list)
    NotIn(Vec<T>),
    /// BETWEEN a AND b
    Between(T, T),
    /// NOT BETWEEN a AND b
    NotBetween(T, T),
}

impl<T> Op<T> {
    /// Create an equality condition.
    pub fn eq(val: T) -> Self {
        Op::Eq(val)
    }

    /// Create a not-equal condition.
    pub fn ne(val: T) -> Self {
        Op::Ne(val)
    }

    /// Create a greater-than condition.
    pub fn gt(val: T) -> Self {
        Op::Gt(val)
    }

    /// Create a greater-than-or-equal condition.
    pub fn gte(val: T) -> Self {
        Op::Gte(val)
    }

    /// Create a less-than condition.
    pub fn lt(val: T) -> Self {
        Op::Lt(val)
    }

    /// Create a less-than-or-equal condition.
    pub fn lte(val: T) -> Self {
        Op::Lte(val)
    }

    /// Create a LIKE pattern match condition.
    pub fn like(val: T) -> Self {
        Op::Like(val)
    }

    /// Create a case-insensitive ILIKE pattern match condition.
    pub fn ilike(val: T) -> Self {
        Op::Ilike(val)
    }

    /// Create a NOT LIKE pattern match condition.
    pub fn not_like(val: T) -> Self {
        Op::NotLike(val)
    }

    /// Create a NOT ILIKE pattern match condition.
    pub fn not_ilike(val: T) -> Self {
        Op::NotIlike(val)
    }

    /// Create an IS NULL condition.
    pub fn is_null() -> Self {
        Op::IsNull
    }

    /// Create an IS NOT NULL condition.
    pub fn is_not_null() -> Self {
        Op::IsNotNull
    }

    /// Create an IN (list) condition.
    pub fn in_list(vals: Vec<T>) -> Self {
        Op::In(vals)
    }

    /// Create a NOT IN (list) condition.
    pub fn not_in(vals: Vec<T>) -> Self {
        Op::NotIn(vals)
    }

    /// Create a BETWEEN condition.
    pub fn between(from: T, to: T) -> Self {
        Op::Between(from, to)
    }

    /// Create a NOT BETWEEN condition.
    pub fn not_between(from: T, to: T) -> Self {
        Op::NotBetween(from, to)
    }
}

/// Internal enum to hold boxed values for conditions.
#[derive(Debug, Clone)]
enum ConditionValue {
    Single(Arc<dyn ToSql + Send + Sync>),
    Pair(Arc<dyn ToSql + Send + Sync>, Arc<dyn ToSql + Send + Sync>),
    List(Vec<Arc<dyn ToSql + Send + Sync>>),
    None,
}

/// A controlled SQL template part.
///
/// This is used internally by specific helpers (e.g. `ANY/ALL`, full-text search)
/// where the shape cannot be expressed as `col <op> $n`.
#[derive(Debug, Clone)]
enum ConditionPart {
    Raw(&'static str),
    Ident(Ident),
    Param(Arc<dyn ToSql + Send + Sync>),
}

/// Internal representation of a [`Condition`].
#[derive(Debug, Clone)]
enum ConditionInner {
    /// Raw SQL condition (escape hatch).
    ///
    /// # Safety
    /// Be careful with SQL injection when using raw conditions.
    Raw(String),
    /// A structured condition over a validated identifier.
    Expr {
        column: Ident,
        operator: &'static str,
        value: ConditionValue,
    },
    /// A structured tuple comparison condition over validated identifiers.
    ///
    /// This is primarily used for keyset/cursor pagination:
    /// `WHERE (a, b) < ($1, $2)` / `WHERE (a, b) > ($1, $2)`.
    Tuple2 {
        columns: (Ident, Ident),
        operator: &'static str,
        values: (Arc<dyn ToSql + Send + Sync>, Arc<dyn ToSql + Send + Sync>),
    },
    /// A structured condition expressed as a controlled sequence of parts.
    ///
    /// This is an internal escape hatch for specific Postgres operators/functions
    /// that don't fit `Expr { column, operator, value }`.
    Parts(Vec<ConditionPart>),
}

/// A query condition primitive used by builders.
#[derive(Debug, Clone)]
pub struct Condition(ConditionInner);

impl Condition {
    /// Create a new structured condition from a column identifier and operator.
    pub fn new<I, T>(column: I, op: Op<T>) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        let column = column.into_ident()?;
        let (operator, value) = match op {
            Op::Eq(v) => ("=", ConditionValue::Single(Arc::new(v))),
            Op::Ne(v) => ("!=", ConditionValue::Single(Arc::new(v))),
            Op::Gt(v) => (">", ConditionValue::Single(Arc::new(v))),
            Op::Gte(v) => (">=", ConditionValue::Single(Arc::new(v))),
            Op::Lt(v) => ("<", ConditionValue::Single(Arc::new(v))),
            Op::Lte(v) => ("<=", ConditionValue::Single(Arc::new(v))),
            Op::Like(v) => ("LIKE", ConditionValue::Single(Arc::new(v))),
            Op::Ilike(v) => ("ILIKE", ConditionValue::Single(Arc::new(v))),
            Op::NotLike(v) => ("NOT LIKE", ConditionValue::Single(Arc::new(v))),
            Op::NotIlike(v) => ("NOT ILIKE", ConditionValue::Single(Arc::new(v))),
            Op::IsNull => ("IS NULL", ConditionValue::None),
            Op::IsNotNull => ("IS NOT NULL", ConditionValue::None),
            Op::In(vals) => {
                let values: Vec<Arc<dyn ToSql + Send + Sync>> =
                    vals.into_iter().map(|v| Arc::new(v) as _).collect();
                ("IN", ConditionValue::List(values))
            }
            Op::NotIn(vals) => {
                let values: Vec<Arc<dyn ToSql + Send + Sync>> =
                    vals.into_iter().map(|v| Arc::new(v) as _).collect();
                ("NOT IN", ConditionValue::List(values))
            }
            Op::Between(from, to) => (
                "BETWEEN",
                ConditionValue::Pair(Arc::new(from), Arc::new(to)),
            ),
            Op::NotBetween(from, to) => (
                "NOT BETWEEN",
                ConditionValue::Pair(Arc::new(from), Arc::new(to)),
            ),
        };

        Ok(Condition(ConditionInner::Expr {
            column,
            operator,
            value,
        }))
    }

    /// Create a raw SQL condition.
    ///
    /// # Safety
    /// Be careful with SQL injection when using raw conditions.
    pub fn raw(sql: impl Into<String>) -> Self {
        Condition(ConditionInner::Raw(sql.into()))
    }

    pub(crate) fn cmp_dyn(
        column: Ident,
        operator: &'static str,
        value: Arc<dyn ToSql + Send + Sync>,
    ) -> Self {
        Condition(ConditionInner::Expr {
            column,
            operator,
            value: ConditionValue::Single(value),
        })
    }

    pub(crate) fn tuple2_cmp_dyn(
        a: Ident,
        b: Ident,
        operator: &'static str,
        va: Arc<dyn ToSql + Send + Sync>,
        vb: Arc<dyn ToSql + Send + Sync>,
    ) -> Self {
        Condition(ConditionInner::Tuple2 {
            columns: (a, b),
            operator,
            values: (va, vb),
        })
    }

    // ==================== Convenience constructors ====================

    /// Create an equality condition: column = value
    pub fn eq<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Eq(value))
    }

    /// Create an inequality condition: column != value
    pub fn ne<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Ne(value))
    }

    /// Create a greater-than condition: column > value
    pub fn gt<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Gt(value))
    }

    /// Create a greater-than-or-equal condition: column >= value
    pub fn gte<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Gte(value))
    }

    /// Create a less-than condition: column < value
    pub fn lt<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Lt(value))
    }

    /// Create a less-than-or-equal condition: column <= value
    pub fn lte<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Lte(value))
    }

    /// Create a LIKE condition: column LIKE pattern
    pub fn like<I, T>(column: I, pattern: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Like(pattern))
    }

    /// Create a case-insensitive ILIKE condition: column ILIKE pattern
    pub fn ilike<I, T>(column: I, pattern: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Ilike(pattern))
    }

    /// Create a NOT LIKE condition: column NOT LIKE pattern
    pub fn not_like<I, T>(column: I, pattern: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::NotLike(pattern))
    }

    /// Create a NOT ILIKE condition: column NOT ILIKE pattern
    pub fn not_ilike<I, T>(column: I, pattern: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::NotIlike(pattern))
    }

    /// Create an IS NULL condition: column IS NULL
    pub fn is_null<I>(column: I) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        Ok(Condition(ConditionInner::Expr {
            column: column.into_ident()?,
            operator: "IS NULL",
            value: ConditionValue::None,
        }))
    }

    /// Create an IS NOT NULL condition: column IS NOT NULL
    pub fn is_not_null<I>(column: I) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        Ok(Condition(ConditionInner::Expr {
            column: column.into_ident()?,
            operator: "IS NOT NULL",
            value: ConditionValue::None,
        }))
    }

    /// Create an IN condition: column IN (values...)
    pub fn in_list<I, T>(column: I, values: Vec<T>) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::In(values))
    }

    /// Create a NOT IN condition: column NOT IN (values...)
    pub fn not_in<I, T>(column: I, values: Vec<T>) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::NotIn(values))
    }

    /// Create a BETWEEN condition: column BETWEEN from AND to
    pub fn between<I, T>(column: I, from: T, to: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::Between(from, to))
    }

    /// Create a NOT BETWEEN condition: column NOT BETWEEN from AND to
    pub fn not_between<I, T>(column: I, from: T, to: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Self::new(column, Op::NotBetween(from, to))
    }

    /// Create a NULL-safe comparison: column IS DISTINCT FROM value
    pub fn is_distinct_from<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(
            column.into_ident()?,
            "IS DISTINCT FROM",
            Arc::new(value),
        ))
    }

    /// Create a NULL-safe comparison: column IS NOT DISTINCT FROM value
    pub fn is_not_distinct_from<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(
            column.into_ident()?,
            "IS NOT DISTINCT FROM",
            Arc::new(value),
        ))
    }

    /// Create a "contains" condition: column @> value
    pub fn contains<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "@>", Arc::new(value)))
    }

    /// Create a "contained by" condition: column <@ value
    pub fn contained_by<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "<@", Arc::new(value)))
    }

    /// Create an "overlaps" condition: column && value
    pub fn overlaps<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "&&", Arc::new(value)))
    }

    /// Create a "strictly left of" condition for ranges: column << value
    pub fn range_left_of<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "<<", Arc::new(value)))
    }

    /// Create a "strictly right of" condition for ranges: column >> value
    pub fn range_right_of<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, ">>", Arc::new(value)))
    }

    /// Create an "adjacent to" condition for ranges: column -|- value
    pub fn range_adjacent<I, T>(column: I, value: T) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "-|-", Arc::new(value)))
    }

    /// Create a jsonb key existence condition: column ? key
    pub fn has_key<I>(column: I, key: impl Into<String>) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        Ok(Self::cmp_dyn(
            column.into_ident()?,
            "?",
            Arc::new(key.into()),
        ))
    }

    /// Create a jsonb "has any keys" condition: column ?| keys
    ///
    /// `keys` is bound as a `text[]`.
    pub fn has_any_keys<I>(column: I, keys: Vec<String>) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "?|", Arc::new(keys)))
    }

    /// Create a jsonb "has all keys" condition: column ?& keys
    ///
    /// `keys` is bound as a `text[]`.
    pub fn has_all_keys<I>(column: I, keys: Vec<String>) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        Ok(Self::cmp_dyn(column.into_ident()?, "?&", Arc::new(keys)))
    }

    /// Create a Postgres `= ANY($n)` condition, binding the values as a single array parameter.
    pub fn eq_any<I, T>(column: I, values: Vec<T>) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        let column = column.into_ident()?;
        let values: Arc<dyn ToSql + Send + Sync> = Arc::new(values);
        Ok(Condition(ConditionInner::Parts(vec![
            ConditionPart::Ident(column),
            ConditionPart::Raw(" = ANY("),
            ConditionPart::Param(values),
            ConditionPart::Raw(")"),
        ])))
    }

    /// Create a Postgres `!= ALL($n)` condition, binding the values as a single array parameter.
    pub fn ne_all<I, T>(column: I, values: Vec<T>) -> OrmResult<Self>
    where
        I: IntoIdent,
        T: ToSql + Send + Sync + 'static,
    {
        let column = column.into_ident()?;
        let values: Arc<dyn ToSql + Send + Sync> = Arc::new(values);
        Ok(Condition(ConditionInner::Parts(vec![
            ConditionPart::Ident(column),
            ConditionPart::Raw(" != ALL("),
            ConditionPart::Param(values),
            ConditionPart::Raw(")"),
        ])))
    }

    /// Create a Postgres full-text search condition:
    /// `to_tsvector(column) @@ plainto_tsquery($n)`
    pub fn ts_match<I>(column: I, query: impl Into<String>) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        let column = column.into_ident()?;
        let query: Arc<dyn ToSql + Send + Sync> = Arc::new(query.into());
        Ok(Condition(ConditionInner::Parts(vec![
            ConditionPart::Raw("to_tsvector("),
            ConditionPart::Ident(column),
            ConditionPart::Raw(") @@ plainto_tsquery("),
            ConditionPart::Param(query),
            ConditionPart::Raw(")"),
        ])))
    }

    /// Create a Postgres full-text search condition using an explicit language/config:
    /// `to_tsvector($lang::regconfig, column) @@ plainto_tsquery($lang::regconfig, $query)`
    ///
    /// Note: the language is bound (not interpolated) to avoid SQL injection.
    pub fn ts_match_lang<I>(
        lang: impl Into<String>,
        column: I,
        query: impl Into<String>,
    ) -> OrmResult<Self>
    where
        I: IntoIdent,
    {
        let column = column.into_ident()?;
        let lang: Arc<dyn ToSql + Send + Sync> = Arc::new(lang.into());
        let query: Arc<dyn ToSql + Send + Sync> = Arc::new(query.into());
        Ok(Condition(ConditionInner::Parts(vec![
            ConditionPart::Raw("to_tsvector("),
            ConditionPart::Param(lang.clone()),
            ConditionPart::Raw("::regconfig, "),
            ConditionPart::Ident(column),
            ConditionPart::Raw(") @@ plainto_tsquery("),
            ConditionPart::Param(lang),
            ConditionPart::Raw("::regconfig, "),
            ConditionPart::Param(query),
            ConditionPart::Raw(")"),
        ])))
    }

    /// Build the SQL fragment and return parameter references.
    pub fn build(&self, param_idx: &mut usize) -> (String, Vec<&(dyn ToSql + Sync)>) {
        match &self.0 {
            ConditionInner::Raw(s) => (s.clone(), Vec::new()),
            ConditionInner::Parts(parts) => {
                let mut out = String::new();
                let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
                for part in parts {
                    match part {
                        ConditionPart::Raw(s) => out.push_str(s),
                        ConditionPart::Ident(ident) => ident.write_sql(&mut out),
                        ConditionPart::Param(v) => {
                            *param_idx += 1;
                            out.push('$');
                            use std::fmt::Write;
                            let _ = write!(&mut out, "{}", *param_idx);
                            params.push(&**v as &(dyn ToSql + Sync));
                        }
                    }
                }
                (out, params)
            }
            ConditionInner::Expr {
                column,
                operator,
                value,
            } => {
                let col = column.to_sql();
                match value {
                    ConditionValue::Single(v) => {
                        *param_idx += 1;
                        let sql = format!("{} {} ${}", col, operator, *param_idx);
                        (sql, vec![&**v as &(dyn ToSql + Sync)])
                    }
                    ConditionValue::Pair(a, b) => {
                        *param_idx += 1;
                        let p1 = *param_idx;
                        *param_idx += 1;
                        let p2 = *param_idx;
                        let sql = format!("{col} {operator} ${p1} AND ${p2}");
                        (
                            sql,
                            vec![&**a as &(dyn ToSql + Sync), &**b as &(dyn ToSql + Sync)],
                        )
                    }
                    ConditionValue::List(vals) => {
                        if vals.is_empty() {
                            // Empty IN list - always false / true
                            if *operator == "IN" {
                                return ("1=0".to_string(), Vec::new());
                            }
                            return ("1=1".to_string(), Vec::new());
                        }
                        let mut placeholders = Vec::new();
                        let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
                        for v in vals {
                            *param_idx += 1;
                            placeholders.push(format!("${}", *param_idx));
                            params.push(&**v as &(dyn ToSql + Sync));
                        }
                        let sql = format!("{} {} ({})", col, operator, placeholders.join(", "));
                        (sql, params)
                    }
                    ConditionValue::None => {
                        let sql = format!("{col} {operator}");
                        (sql, Vec::new())
                    }
                }
            }
            ConditionInner::Tuple2 {
                columns: (a, b),
                operator,
                values: (va, vb),
            } => {
                *param_idx += 1;
                let p1 = *param_idx;
                *param_idx += 1;
                let p2 = *param_idx;
                let sql = format!(
                    "({}, {}) {} (${}, ${})",
                    a.to_sql(),
                    b.to_sql(),
                    operator,
                    p1,
                    p2
                );
                (
                    sql,
                    vec![&**va as &(dyn ToSql + Sync), &**vb as &(dyn ToSql + Sync)],
                )
            }
        }
    }

    /// Append this condition into a [`Sql`] builder.
    ///
    /// This lets you reuse the same `Condition` primitives with the SQL-first [`Sql`] builder:
    /// placeholders are generated by `Sql`, and values are carried over safely.
    pub fn append_to_sql(&self, sql: &mut Sql) {
        match &self.0 {
            ConditionInner::Raw(s) => {
                sql.push(s);
            }
            ConditionInner::Parts(parts) => {
                for part in parts {
                    match part {
                        ConditionPart::Raw(s) => {
                            sql.push(s);
                        }
                        ConditionPart::Ident(ident) => {
                            sql.push_ident_ref(ident);
                        }
                        ConditionPart::Param(v) => {
                            sql.push_bind_value(v.clone());
                        }
                    }
                }
            }
            ConditionInner::Expr {
                column,
                operator,
                value,
            } => match value {
                ConditionValue::List(vals) if vals.is_empty() => {
                    // Empty IN list - always false / true, matching `build()`.
                    if *operator == "IN" {
                        sql.push("1=0");
                    } else {
                        sql.push("1=1");
                    }
                }
                ConditionValue::Single(v) => {
                    sql.push_ident_ref(column);
                    sql.push(" ");
                    sql.push(operator);
                    sql.push(" ");
                    sql.push_bind_value(v.clone());
                }
                ConditionValue::Pair(a, b) => {
                    sql.push_ident_ref(column);
                    sql.push(" ");
                    sql.push(operator);
                    sql.push(" ");
                    sql.push_bind_value(a.clone());
                    sql.push(" AND ");
                    sql.push_bind_value(b.clone());
                }
                ConditionValue::List(vals) => {
                    sql.push_ident_ref(column);
                    sql.push(" ");
                    sql.push(operator);
                    sql.push(" (");
                    for (i, v) in vals.iter().enumerate() {
                        if i > 0 {
                            sql.push(", ");
                        }
                        sql.push_bind_value(v.clone());
                    }
                    sql.push(")");
                }
                ConditionValue::None => {
                    sql.push_ident_ref(column);
                    sql.push(" ");
                    sql.push(operator);
                }
            },
            ConditionInner::Tuple2 {
                columns: (a, b),
                operator,
                values: (va, vb),
            } => {
                sql.push("(");
                sql.push_ident_ref(a);
                sql.push(", ");
                sql.push_ident_ref(b);
                sql.push(") ");
                sql.push(operator);
                sql.push(" (");
                sql.push_bind_value(va.clone());
                sql.push(", ");
                sql.push_bind_value(vb.clone());
                sql.push(")");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_condition_sql(cond: &Condition, expected_sql: &str, expected_params: usize) {
        let mut idx = 0;
        let (sql, params) = cond.build(&mut idx);
        assert_eq!(sql, expected_sql);
        assert_eq!(params.len(), expected_params);
        assert_eq!(idx, expected_params);

        let mut b = Sql::empty();
        cond.append_to_sql(&mut b);
        assert_eq!(b.to_sql(), expected_sql);
        assert_eq!(b.params_ref().len(), expected_params);
    }

    #[test]
    fn condition_is_distinct_from() {
        let cond = Condition::is_distinct_from("deleted_at", Option::<i64>::None).unwrap();
        assert_condition_sql(&cond, "deleted_at IS DISTINCT FROM $1", 1);
    }

    #[test]
    fn condition_contains_jsonb() {
        let cond = Condition::contains("metadata", serde_json::json!({"env": "prod"})).unwrap();
        assert_condition_sql(&cond, "metadata @> $1", 1);
    }

    #[test]
    fn condition_has_key() {
        let cond = Condition::has_key("metadata", "user_id").unwrap();
        assert_condition_sql(&cond, "metadata ? $1", 1);
    }

    #[test]
    fn condition_has_any_keys() {
        let cond =
            Condition::has_any_keys("metadata", vec!["a".to_string(), "b".to_string()]).unwrap();
        assert_condition_sql(&cond, "metadata ?| $1", 1);
    }

    #[test]
    fn condition_eq_any() {
        let cond = Condition::eq_any("user_id", vec![1_i64, 2, 3]).unwrap();
        assert_condition_sql(&cond, "user_id = ANY($1)", 1);
    }

    #[test]
    fn condition_ne_all() {
        let cond = Condition::ne_all("user_id", vec![1_i64, 2, 3]).unwrap();
        assert_condition_sql(&cond, "user_id != ALL($1)", 1);
    }

    #[test]
    fn condition_ts_match() {
        let cond = Condition::ts_match("content", "hello world").unwrap();
        assert_condition_sql(&cond, "to_tsvector(content) @@ plainto_tsquery($1)", 1);
    }

    #[test]
    fn condition_ts_match_lang() {
        let cond = Condition::ts_match_lang("english", "content", "hello world").unwrap();
        assert_condition_sql(
            &cond,
            "to_tsvector($1::regconfig, content) @@ plainto_tsquery($2::regconfig, $3)",
            3,
        );
    }

    #[test]
    fn condition_range_left_of() {
        let cond = Condition::range_left_of("during", 1_i32).unwrap();
        assert_condition_sql(&cond, "during << $1", 1);
    }

    #[test]
    fn condition_range_right_of() {
        let cond = Condition::range_right_of("during", 1_i32).unwrap();
        assert_condition_sql(&cond, "during >> $1", 1);
    }

    #[test]
    fn condition_range_adjacent() {
        let cond = Condition::range_adjacent("during", 1_i32).unwrap();
        assert_condition_sql(&cond, "during -|- $1", 1);
    }

    #[test]
    fn condition_build_respects_param_idx() {
        let c1 = Condition::eq_any("id", vec![1_i32, 2]).unwrap();
        let c2 = Condition::is_distinct_from("a", 1_i32).unwrap();

        let mut idx = 0;
        let (sql1, p1) = c1.build(&mut idx);
        assert_eq!(sql1, "id = ANY($1)");
        assert_eq!(p1.len(), 1);

        let (sql2, p2) = c2.build(&mut idx);
        assert_eq!(sql2, "a IS DISTINCT FROM $2");
        assert_eq!(p2.len(), 1);
        assert_eq!(idx, 2);
    }
}
