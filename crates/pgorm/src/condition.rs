//! Query condition types for dynamic queries.
//!
//! This module provides `Op` (operator) and `Condition` types for building
//! flexible WHERE clauses with various comparison operators.

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
#[derive(Debug)]
enum ConditionValue {
    Single(Box<dyn ToSql + Send + Sync>),
    Pair(Box<dyn ToSql + Send + Sync>, Box<dyn ToSql + Send + Sync>),
    List(Vec<Box<dyn ToSql + Send + Sync>>),
    None,
}

impl Clone for ConditionValue {
    fn clone(&self) -> Self {
        // ConditionValue cannot be cloned due to dyn ToSql
        // We'll handle this in Condition's Clone impl
        panic!("ConditionValue cannot be cloned directly")
    }
}

/// A query condition with column, operator, and values.
#[derive(Debug)]
pub struct Condition {
    column: String,
    operator: &'static str,
    value: ConditionValue,
    is_raw: bool,
    raw_sql: Option<String>,
}

impl Clone for Condition {
    fn clone(&self) -> Self {
        if self.is_raw {
            Condition {
                column: self.column.clone(),
                operator: self.operator,
                value: ConditionValue::None,
                is_raw: true,
                raw_sql: self.raw_sql.clone(),
            }
        } else {
            // For non-raw conditions, we can't clone the boxed values
            // This is a limitation - conditions with values cannot be cloned
            panic!("Condition with values cannot be cloned. Use raw conditions or rebuild the query.")
        }
    }
}

impl Condition {
    /// Create a new condition from column and operator.
    pub fn new<T>(column: &str, op: Op<T>) -> Self
    where
        T: ToSql + Send + Sync + 'static,
    {
        let (operator, value) = match op {
            Op::Eq(v) => ("=", ConditionValue::Single(Box::new(v))),
            Op::Ne(v) => ("!=", ConditionValue::Single(Box::new(v))),
            Op::Gt(v) => (">", ConditionValue::Single(Box::new(v))),
            Op::Gte(v) => (">=", ConditionValue::Single(Box::new(v))),
            Op::Lt(v) => ("<", ConditionValue::Single(Box::new(v))),
            Op::Lte(v) => ("<=", ConditionValue::Single(Box::new(v))),
            Op::Like(v) => ("LIKE", ConditionValue::Single(Box::new(v))),
            Op::Ilike(v) => ("ILIKE", ConditionValue::Single(Box::new(v))),
            Op::NotLike(v) => ("NOT LIKE", ConditionValue::Single(Box::new(v))),
            Op::NotIlike(v) => ("NOT ILIKE", ConditionValue::Single(Box::new(v))),
            Op::IsNull => ("IS NULL", ConditionValue::None),
            Op::IsNotNull => ("IS NOT NULL", ConditionValue::None),
            Op::In(vals) => {
                let boxed: Vec<Box<dyn ToSql + Send + Sync>> =
                    vals.into_iter().map(|v| Box::new(v) as _).collect();
                ("IN", ConditionValue::List(boxed))
            }
            Op::NotIn(vals) => {
                let boxed: Vec<Box<dyn ToSql + Send + Sync>> =
                    vals.into_iter().map(|v| Box::new(v) as _).collect();
                ("NOT IN", ConditionValue::List(boxed))
            }
            Op::Between(from, to) => (
                "BETWEEN",
                ConditionValue::Pair(Box::new(from), Box::new(to)),
            ),
            Op::NotBetween(from, to) => (
                "NOT BETWEEN",
                ConditionValue::Pair(Box::new(from), Box::new(to)),
            ),
        };

        Condition {
            column: column.to_string(),
            operator,
            value,
            is_raw: false,
            raw_sql: None,
        }
    }

    /// Create a raw SQL condition.
    ///
    /// # Safety
    /// Be careful with SQL injection when using raw conditions.
    pub fn raw(sql: &str) -> Self {
        Condition {
            column: String::new(),
            operator: "",
            value: ConditionValue::None,
            is_raw: true,
            raw_sql: Some(sql.to_string()),
        }
    }

    /// Build the SQL fragment and return parameter references.
    pub fn build(&self, param_idx: &mut usize) -> (String, Vec<&(dyn ToSql + Sync)>) {
        if self.is_raw {
            return (self.raw_sql.clone().unwrap_or_default(), Vec::new());
        }

        match &self.value {
            ConditionValue::Single(v) => {
                *param_idx += 1;
                let sql = format!("{} {} ${}", self.column, self.operator, *param_idx);
                (sql, vec![&**v as &(dyn ToSql + Sync)])
            }
            ConditionValue::Pair(a, b) => {
                *param_idx += 1;
                let p1 = *param_idx;
                *param_idx += 1;
                let p2 = *param_idx;
                let sql = format!("{} {} ${} AND ${}", self.column, self.operator, p1, p2);
                (
                    sql,
                    vec![&**a as &(dyn ToSql + Sync), &**b as &(dyn ToSql + Sync)],
                )
            }
            ConditionValue::List(vals) => {
                if vals.is_empty() {
                    // Empty IN list - always false
                    if self.operator == "IN" {
                        return ("1=0".to_string(), Vec::new());
                    } else {
                        // NOT IN empty list - always true
                        return ("1=1".to_string(), Vec::new());
                    }
                }
                let mut placeholders = Vec::new();
                let mut params: Vec<&(dyn ToSql + Sync)> = Vec::new();
                for v in vals {
                    *param_idx += 1;
                    placeholders.push(format!("${}", *param_idx));
                    params.push(&**v as &(dyn ToSql + Sync));
                }
                let sql = format!(
                    "{} {} ({})",
                    self.column,
                    self.operator,
                    placeholders.join(", ")
                );
                (sql, params)
            }
            ConditionValue::None => {
                // IS NULL / IS NOT NULL
                let sql = format!("{} {}", self.column, self.operator);
                (sql, Vec::new())
            }
        }
    }
}
